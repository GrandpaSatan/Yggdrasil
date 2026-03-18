/// SDR-based skill cache for instant tool dispatch from raw audio.
///
/// When a voice command successfully triggers a tool call, the raw PCM audio
/// is fingerprinted into a 256-bit SDR (via Mel spectrogram → SHA-256) and
/// cached alongside the tool name and arguments.
///
/// On subsequent utterances, the audio SDR is computed directly from the PCM
/// buffer (~1ms, pure CPU) and matched against cached skills via Hamming
/// similarity. A cache hit skips LLM inference entirely.
///
/// This follows the same pattern as `ygg-voice::sdr_commands::SdrCommandRegistry`
/// but learns dynamically from successful tool calls instead of pre-registered commands.
use std::sync::Arc;
use std::time::Instant;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use ygg_domain::sdr::{self, Sdr};

/// Minimum Hamming similarity for a cache hit.
const DEFAULT_THRESHOLD: f64 = 0.85;

// Mel spectrogram constants (matching ygg-voice/mel.rs Whisper params).
const SAMPLE_RATE: usize = 16_000;
const FFT_SIZE: usize = 400;
const HOP_LENGTH: usize = 160;
const MEL_BINS: usize = 80;
const FREQ_BINS: usize = FFT_SIZE / 2 + 1;
/// Fingerprint window: first 2 seconds of audio.
const FINGERPRINT_SAMPLES: usize = SAMPLE_RATE * 2;

/// A cached skill: audio SDR → tool call mapping.
#[derive(Debug, Clone)]
pub struct CachedSkill {
    pub sdr: Sdr,
    pub label: String,
    pub tool_name: String,
    pub tool_args: JsonValue,
    pub hit_count: u32,
    pub last_used: Instant,
}

/// Result of a skill cache lookup.
pub struct SkillMatch {
    pub tool_name: String,
    pub tool_args: JsonValue,
    pub similarity: f64,
}

/// Thread-safe skill cache with audio-SDR matching.
///
/// Pre-computes the Mel filterbank, Hann window, and FFT plan at construction time.
pub struct SkillCache {
    skills: RwLock<Vec<CachedSkill>>,
    threshold: f64,
    filterbank: Vec<Vec<f32>>,
    hann_window: Vec<f32>,
    /// Pre-computed FFT plan for `FFT_SIZE`. `Arc<dyn Fft>` is `Send + Sync`.
    fft_plan: Arc<dyn Fft<f32>>,
}

impl Default for SkillCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillCache {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft_plan = planner.plan_fft_forward(FFT_SIZE);
        Self {
            skills: RwLock::new(Vec::new()),
            threshold: DEFAULT_THRESHOLD,
            filterbank: build_mel_filterbank(),
            hann_window: build_hann_window(),
            fft_plan,
        }
    }

    /// Compute a 256-bit SDR fingerprint from raw i16 PCM audio.
    ///
    /// Uses the first 2 seconds: Mel spectrogram → average energy → quantize → SHA-256.
    /// ~1ms on CPU, no network, no models.
    pub fn fingerprint(&self, audio: &[i16]) -> Sdr {
        // Convert i16 to f32.
        let f32_audio: Vec<f32> = audio.iter().map(|&s| s as f32 / 32768.0).collect();

        // Pad or truncate to 2 seconds.
        let mut padded = vec![0.0f32; FINGERPRINT_SAMPLES];
        let copy_len = f32_audio.len().min(FINGERPRINT_SAMPLES);
        padded[..copy_len].copy_from_slice(&f32_audio[..copy_len]);

        let num_frames = FINGERPRINT_SAMPLES.saturating_sub(FFT_SIZE) / HOP_LENGTH + 1;
        if num_frames == 0 {
            return sdr::ZERO;
        }

        let fft = &self.fft_plan;
        let mut fft_buffer = vec![Complex::new(0.0f32, 0.0f32); FFT_SIZE];

        // Average mel energy across all frames.
        let mut avg_mel = vec![0.0f32; MEL_BINS];

        for frame_idx in 0..num_frames {
            let start = frame_idx * HOP_LENGTH;
            if start + FFT_SIZE > padded.len() {
                break;
            }

            for i in 0..FFT_SIZE {
                fft_buffer[i] = Complex::new(padded[start + i] * self.hann_window[i], 0.0);
            }
            fft.process(&mut fft_buffer);

            let power: Vec<f32> = fft_buffer[..FREQ_BINS]
                .iter()
                .map(|c| c.norm_sqr())
                .collect();

            for (mel_idx, filter) in self.filterbank.iter().enumerate() {
                let mut energy = 0.0f32;
                for (freq_idx, &weight) in filter.iter().enumerate() {
                    energy += weight * power[freq_idx];
                }
                avg_mel[mel_idx] += energy;
            }
        }

        // Quantize to u8 and hash.
        let scale = 1.0 / num_frames as f32;
        let mut quantized = vec![0u8; MEL_BINS];
        for i in 0..MEL_BINS {
            let log_val = (avg_mel[i] * scale).max(1e-10).log10();
            let normalized = ((log_val + 10.0) / 12.0).clamp(0.0, 1.0);
            quantized[i] = (normalized * 255.0) as u8;
        }

        let hash = Sha256::digest(&quantized);
        sdr::from_bytes(&hash).unwrap_or(sdr::ZERO)
    }

    /// Query the cache for a matching skill.
    ///
    /// Uses a read lock for the linear scan, then a write lock only when a hit
    /// is found to update `hit_count` and `last_used`. This unblocks concurrent
    /// readers during the O(N) scan phase. Hit-count updates are best-effort.
    pub async fn match_skill(&self, query_sdr: &Sdr) -> Option<SkillMatch> {
        // Phase 1: read-only scan.
        let best = {
            let guard = self.skills.read().await;
            let mut best_idx = None;
            let mut best_sim = 0.0_f64;
            for (i, skill) in guard.iter().enumerate() {
                let sim = sdr::hamming_similarity(query_sdr, &skill.sdr);
                if sim >= self.threshold && sim > best_sim {
                    best_sim = sim;
                    best_idx = Some(i);
                }
            }
            best_idx.map(|idx| {
                let skill = &guard[idx];
                (idx, skill.tool_name.clone(), skill.tool_args.clone(), best_sim)
            })
        }; // read lock dropped

        // Phase 2: write lock only on hit (best-effort hit tracking).
        if let Some((idx, tool_name, tool_args, similarity)) = best {
            let mut guard = self.skills.write().await;
            if idx < guard.len() {
                guard[idx].hit_count += 1;
                guard[idx].last_used = Instant::now();
            }
            Some(SkillMatch { tool_name, tool_args, similarity })
        } else {
            None
        }
    }

    /// Cache a new skill after a successful tool call.
    pub async fn learn(&self, audio_sdr: Sdr, label: String, tool_name: String, tool_args: JsonValue) {
        let mut guard = self.skills.write().await;

        // Deduplicate near-identical skills.
        for skill in guard.iter_mut() {
            if sdr::hamming_similarity(&audio_sdr, &skill.sdr) >= self.threshold {
                skill.hit_count += 1;
                skill.last_used = Instant::now();
                tracing::debug!(tool = %tool_name, "skill cache: updated existing skill");
                return;
            }
        }

        tracing::info!(
            tool = %tool_name,
            label = %label,
            total = guard.len() + 1,
            "skill cache: learned new skill from audio"
        );

        guard.push(CachedSkill {
            sdr: audio_sdr,
            label,
            tool_name,
            tool_args,
            hit_count: 1,
            last_used: Instant::now(),
        });
    }

    pub async fn len(&self) -> usize {
        self.skills.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.skills.read().await.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────────
// Mel filterbank (matches ygg-voice/mel.rs exactly)
// ─────────────────────────────────────────────────────────────────

fn build_mel_filterbank() -> Vec<Vec<f32>> {
    let sr = SAMPLE_RATE as f32;
    let hz_to_mel = |hz: f32| -> f32 { 2595.0 * (1.0 + hz / 700.0).log10() };
    let mel_to_hz = |mel: f32| -> f32 { 700.0 * (10.0f32.powf(mel / 2595.0) - 1.0) };

    let mel_low = hz_to_mel(0.0);
    let mel_high = hz_to_mel(sr / 2.0);

    let n_points = MEL_BINS + 2;
    let mel_points: Vec<f32> = (0..n_points)
        .map(|i| mel_low + (mel_high - mel_low) * i as f32 / (n_points - 1) as f32)
        .collect();

    let bin_points: Vec<f32> = mel_points
        .iter()
        .map(|&m| mel_to_hz(m) * FFT_SIZE as f32 / sr)
        .collect();

    let mut filterbank = vec![vec![0.0f32; FREQ_BINS]; MEL_BINS];

    for m in 0..MEL_BINS {
        let left = bin_points[m];
        let center = bin_points[m + 1];
        let right = bin_points[m + 2];

        for (k, bin) in filterbank[m].iter_mut().enumerate() {
            let freq = k as f32;
            if freq >= left && freq <= center && center > left {
                *bin = (freq - left) / (center - left);
            } else if freq > center && freq <= right && right > center {
                *bin = (right - freq) / (right - center);
            }
        }
    }

    filterbank
}

fn build_hann_window() -> Vec<f32> {
    (0..FFT_SIZE)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_match_on_empty_cache() {
        let cache = SkillCache::new();
        let silence = vec![0i16; 16000];
        let sdr = cache.fingerprint(&silence);
        assert!(cache.match_skill(&sdr).await.is_none());
    }

    #[tokio::test]
    async fn exact_audio_matches() {
        let cache = SkillCache::new();
        let tone: Vec<i16> = (0..16000)
            .map(|i| ((2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16000.0).sin() * 16000.0) as i16)
            .collect();

        let sdr_val = cache.fingerprint(&tone);
        cache.learn(
            sdr_val,
            "test tone".into(),
            "gaming".into(),
            serde_json::json!({"action": "launch", "vm_name": "harpy"}),
        ).await;

        let query_sdr = cache.fingerprint(&tone);
        let result = cache.match_skill(&query_sdr).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().tool_name, "gaming");
    }

    #[tokio::test]
    async fn different_audio_does_not_match() {
        let cache = SkillCache::new();
        let tone: Vec<i16> = (0..16000)
            .map(|i| ((2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16000.0).sin() * 16000.0) as i16)
            .collect();

        let sdr_val = cache.fingerprint(&tone);
        cache.learn(sdr_val, "tone".into(), "gaming".into(), serde_json::json!({})).await;

        // Very different audio (silence).
        let silence = vec![0i16; 16000];
        let query_sdr = cache.fingerprint(&silence);
        assert!(cache.match_skill(&query_sdr).await.is_none());
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let cache = SkillCache::new();
        let audio: Vec<i16> = (0..16000).map(|i| (i % 1000) as i16).collect();
        let sdr1 = cache.fingerprint(&audio);
        let sdr2 = cache.fingerprint(&audio);
        assert_eq!(sdr1, sdr2);
    }
}
