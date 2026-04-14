//! Three-state novelty verdict: New / Update / Old. Sprint 064 P1.
//!
//! Replaces the binary Sprint 016 dedup gate with a triage that lets the
//! `/api/v1/store` handler decide *server-side* whether to insert, overwrite,
//! or skip — instead of returning 409 and asking the client to retry.

use serde::Serialize;
use std::hash::{DefaultHasher, Hash, Hasher};
use uuid::Uuid;
use ygg_domain::config::{DenseNoveltyConfig, NoveltyConfig};

/// Verdict returned by the Mimir novelty gate.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "verdict", rename_all = "lowercase")]
pub enum NoveltyVerdict {
    /// No near-duplicate; insert as a new engram.
    New,
    /// Near-duplicate exists with meaningfully different content; overwrite in place.
    Update {
        id: Uuid,
        previous_cause: String,
        previous_effect: String,
    },
    /// Near-identical engram already exists; skip the write.
    Old { id: Uuid },
}

/// Classify a candidate engram against its nearest neighbour in the SDR index.
///
/// Order of precedence: Old → Update → New. The `Old` check requires both the
/// similarity floor *and* near-identical effect text (whitespace-normalised
/// equality OR Levenshtein distance within tolerance) so high-similarity
/// rewrites are still routed to `Update`.
pub fn classify_novelty(
    similarity: f64,
    new_effect: &str,
    existing_id: Uuid,
    existing_cause: &str,
    existing_effect: &str,
    cfg: &NoveltyConfig,
) -> NoveltyVerdict {
    if similarity >= cfg.old_threshold {
        let normalized_match = normalized(new_effect) == normalized(existing_effect);
        let near_match = levenshtein(new_effect, existing_effect, cfg.levenshtein_tolerance)
            <= cfg.levenshtein_tolerance;
        if normalized_match || near_match {
            return NoveltyVerdict::Old { id: existing_id };
        }
    }
    if similarity >= cfg.update_threshold {
        return NoveltyVerdict::Update {
            id: existing_id,
            previous_cause: existing_cause.to_owned(),
            previous_effect: existing_effect.to_owned(),
        };
    }
    NoveltyVerdict::New
}

/// Verdict returned by the Tier 1 dense cosine classifier (Sprint 067 Phase 1).
///
/// Unlike [`NoveltyVerdict`] this carries a dedicated `Ambiguous` variant for the
/// borderline cosine band (between `ambiguous_floor` and `update_threshold`) where
/// the Phase 2 handler escalates to the store-gate LLM (Tier 2). The SDR-based
/// [`classify_novelty`] remains in the codebase as the panic fallback when the
/// dense path fails.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "verdict", rename_all = "lowercase")]
pub enum DenseVerdict {
    /// No near-duplicate in the dense index; insert as a new engram.
    New,
    /// Near-duplicate with meaningfully different content; overwrite in place.
    Update {
        id: Uuid,
        previous_cause: String,
        previous_effect: String,
    },
    /// Near-identical engram already exists; skip the write.
    Old { id: Uuid },
    /// Borderline cosine — Phase 2 handler escalates to store_gate LLM (Tier 2).
    Ambiguous { id: Uuid, cosine_sim: f64 },
}

/// Classify a candidate engram against its nearest dense-index neighbour.
///
/// Precedence: `Old` → `Update` → `Ambiguous` → `New`.
///
/// - `Old` requires cosine ≥ `old_threshold` AND normalized text match OR
///   Levenshtein distance within `levenshtein_tolerance`.
/// - `Update` fires at cosine ≥ `update_threshold` when text has diverged.
/// - `Ambiguous` captures the `[ambiguous_floor, update_threshold)` band that
///   Phase 2 will escalate to the Tier 2 store-gate LLM.
/// - `New` is the fallthrough (cosine below `ambiguous_floor`).
///
/// Note: when cosine is extremely high (≥ `old_threshold`) but text diverges
/// wildly, we still route to `Update` rather than `Old` — same escape hatch as
/// [`classify_novelty`]. The embedding says "same concept", the text disagrees,
/// so overwrite wins over skip.
pub fn classify_dense(
    cosine_sim: f64,
    new_effect: &str,
    existing_id: Uuid,
    existing_cause: &str,
    existing_effect: &str,
    cfg: &DenseNoveltyConfig,
) -> DenseVerdict {
    if cosine_sim >= cfg.old_threshold {
        let normalized_match = normalized(new_effect) == normalized(existing_effect);
        let near_match = levenshtein(new_effect, existing_effect, cfg.levenshtein_tolerance)
            <= cfg.levenshtein_tolerance;
        if normalized_match || near_match {
            return DenseVerdict::Old { id: existing_id };
        }
    }
    if cosine_sim >= cfg.update_threshold {
        return DenseVerdict::Update {
            id: existing_id,
            previous_cause: existing_cause.to_owned(),
            previous_effect: existing_effect.to_owned(),
        };
    }
    if cosine_sim >= cfg.ambiguous_floor {
        return DenseVerdict::Ambiguous {
            id: existing_id,
            cosine_sim,
        };
    }
    DenseVerdict::New
}

/// 64-bit SimHash of `cause + "\n" + effect` for Tier 0 content pre-filter.
///
/// Tier 0 of the three-tier novelty gate (Sprint 067 Phase 2): when the new
/// engram's SimHash hamming-distance ≤ 2 from a recent engram's hash, the
/// handler can return `Old` without consulting the dense index. Catches
/// exact/near-exact duplicates without the embedding lookup cost.
///
/// Implementation: token-level Charikar SimHash. The input is concatenated as
/// `"{cause}\n{effect}"`, lowercased, and split on whitespace + ASCII
/// punctuation. Each token is hashed via `std::hash::DefaultHasher`; bits vote
/// `+1`/`-1` into a signed accumulator. The final `u64` has bit `i` set iff
/// the accumulator at position `i` is positive.
pub fn simhash_64(cause: &str, effect: &str) -> u64 {
    let combined = format!("{cause}\n{effect}").to_lowercase();
    let mut accumulator: [i32; 64] = [0; 64];
    let mut token_count: u32 = 0;
    for token in combined.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation()) {
        if token.is_empty() {
            continue;
        }
        token_count += 1;
        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        let hash = hasher.finish();
        for (bit, slot) in accumulator.iter_mut().enumerate() {
            if (hash >> bit) & 1 == 1 {
                *slot += 1;
            } else {
                *slot -= 1;
            }
        }
    }
    // Empty input → stable zero hash. (With no tokens, every accumulator slot
    // stays at 0, which produces the same all-zero output below anyway, but
    // returning early makes the contract explicit for callers.)
    if token_count == 0 {
        return 0;
    }
    let mut out: u64 = 0;
    for (bit, slot) in accumulator.iter().enumerate() {
        if *slot > 0 {
            out |= 1u64 << bit;
        }
    }
    out
}

fn normalized(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Bounded Levenshtein on `chars()`. Returns at most `max + 1` to cap cost.
fn levenshtein(a: &str, b: &str, max: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let len_diff = (a.len() as isize - b.len() as isize).unsigned_abs() as usize;
    if len_diff > max {
        return max + 1;
    }
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m.min(max + 1);
    }
    if m == 0 {
        return n.min(max + 1);
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr: Vec<usize> = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        let mut row_min = curr[0];
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            if curr[j] < row_min {
                row_min = curr[j];
            }
        }
        if row_min > max {
            return max + 1;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m].min(max + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> NoveltyConfig {
        NoveltyConfig {
            old_threshold: 0.98,
            update_threshold: 0.85,
            levenshtein_tolerance: 8,
        }
    }

    #[test]
    fn new_when_similarity_below_update_threshold() {
        let v = classify_novelty(0.7, "anything", Uuid::new_v4(), "c", "e", &cfg());
        assert!(matches!(v, NoveltyVerdict::New));
    }

    #[test]
    fn update_when_similarity_in_band() {
        let id = Uuid::new_v4();
        let v = classify_novelty(
            0.90,
            "completely different content",
            id,
            "old cause",
            "old",
            &cfg(),
        );
        match v {
            NoveltyVerdict::Update {
                id: out_id,
                previous_cause,
                previous_effect,
            } => {
                assert_eq!(out_id, id);
                assert_eq!(previous_cause, "old cause");
                assert_eq!(previous_effect, "old");
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn old_when_high_similarity_and_normalized_equal() {
        let id = Uuid::new_v4();
        let v = classify_novelty(0.99, "Hello World", id, "c", "  hello   world\n", &cfg());
        assert!(matches!(v, NoveltyVerdict::Old { id: x } if x == id));
    }

    #[test]
    fn old_when_levenshtein_within_tolerance() {
        let id = Uuid::new_v4();
        let v = classify_novelty(0.99, "abc", id, "c", "abd", &cfg());
        assert!(matches!(v, NoveltyVerdict::Old { id: x } if x == id));
    }

    #[test]
    fn update_when_high_similarity_but_text_far() {
        let v = classify_novelty(
            0.99,
            "hi",
            Uuid::new_v4(),
            "c",
            "completely unrelated lengthy text here",
            &cfg(),
        );
        assert!(matches!(v, NoveltyVerdict::Update { .. }));
    }

    #[test]
    fn levenshtein_early_exit_long_difference() {
        assert_eq!(levenshtein("a", "abcdefghij", 3), 4);
    }

    // --- Sprint 067 Phase 1: DenseVerdict / classify_dense / simhash_64 ---

    fn dense_cfg() -> DenseNoveltyConfig {
        DenseNoveltyConfig {
            enabled: true,
            old_threshold: 0.97,
            update_threshold: 0.88,
            ambiguous_floor: 0.80,
            levenshtein_tolerance: 8,
        }
    }

    #[test]
    fn new_when_below_ambiguous_floor() {
        let v = classify_dense(
            0.75,
            "anything",
            Uuid::new_v4(),
            "prev cause",
            "prev effect",
            &dense_cfg(),
        );
        assert!(matches!(v, DenseVerdict::New));
    }

    #[test]
    fn ambiguous_in_band() {
        let id = Uuid::new_v4();
        let v = classify_dense(
            0.85,
            "candidate text",
            id,
            "prev cause",
            "prev effect",
            &dense_cfg(),
        );
        match v {
            DenseVerdict::Ambiguous {
                id: out_id,
                cosine_sim,
            } => {
                assert_eq!(out_id, id);
                assert!((cosine_sim - 0.85).abs() < f64::EPSILON);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn update_when_high_cosine_text_differs() {
        let id = Uuid::new_v4();
        let v = classify_dense(
            0.92,
            "completely different content here",
            id,
            "old cause",
            "old effect text entirely",
            &dense_cfg(),
        );
        match v {
            DenseVerdict::Update {
                id: out_id,
                previous_cause,
                previous_effect,
            } => {
                assert_eq!(out_id, id);
                assert_eq!(previous_cause, "old cause");
                assert_eq!(previous_effect, "old effect text entirely");
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn old_when_cosine_high_and_text_match() {
        let id = Uuid::new_v4();
        let v = classify_dense(
            0.99,
            "Hello World",
            id,
            "c",
            "  hello   world\n",
            &dense_cfg(),
        );
        assert!(matches!(v, DenseVerdict::Old { id: x } if x == id));
    }

    #[test]
    fn dense_old_when_levenshtein_within_tolerance() {
        let id = Uuid::new_v4();
        let v = classify_dense(0.99, "abcdefghij", id, "c", "abcdefghxy", &dense_cfg());
        assert!(matches!(v, DenseVerdict::Old { id: x } if x == id));
    }

    #[test]
    fn update_when_cosine_very_high_but_text_far() {
        let v = classify_dense(
            0.99,
            "hi",
            Uuid::new_v4(),
            "c",
            "completely unrelated lengthy text here",
            &dense_cfg(),
        );
        assert!(matches!(v, DenseVerdict::Update { .. }));
    }

    // --- simhash_64 tests ---

    fn hamming64(a: u64, b: u64) -> u32 {
        (a ^ b).count_ones()
    }

    #[test]
    fn simhash_identical_inputs_produce_identical_hash() {
        let a = simhash_64("the quick brown fox", "jumps over the lazy dog");
        let b = simhash_64("the quick brown fox", "jumps over the lazy dog");
        assert_eq!(a, b);
    }

    #[test]
    fn simhash_empty_input_is_stable_zero() {
        // No tokens at all → well-defined zero hash.
        assert_eq!(simhash_64("", ""), 0);
        // Whitespace/punctuation-only input also tokens-to-empty → zero.
        assert_eq!(simhash_64("   ", "\t\n"), 0);
        assert_eq!(simhash_64("...", ",,,"), 0);
    }

    #[test]
    fn simhash_single_word_difference_shifts_few_bits() {
        // Two sentences differing by one token out of many should land close in
        // hamming distance — well under 32 (the random-pair expectation).
        let a = simhash_64(
            "sprint sixty seven phase one",
            "dense cosine gate classifier shipping",
        );
        let b = simhash_64(
            "sprint sixty seven phase two",
            "dense cosine gate classifier shipping",
        );
        let dist = hamming64(a, b);
        assert!(
            dist < 32,
            "single-word edit should stay well under 32 bits, got {dist}"
        );
    }

    #[test]
    fn simhash_very_different_content_has_larger_distance() {
        // Completely disjoint content should land much further apart than the
        // single-word edit above. We don't require exactly 32 (DefaultHasher is
        // not a perfect random oracle) but we do require clear separation.
        let similar_a = simhash_64(
            "sprint sixty seven phase one",
            "dense cosine gate classifier shipping",
        );
        let similar_b = simhash_64(
            "sprint sixty seven phase two",
            "dense cosine gate classifier shipping",
        );
        let near_dist = hamming64(similar_a, similar_b);

        let far_a = simhash_64(
            "sprint sixty seven phase one",
            "dense cosine gate classifier shipping",
        );
        let far_b = simhash_64(
            "proxmox lxc container gpu passthrough",
            "morrigan llama server tensor split cuda",
        );
        let far_dist = hamming64(far_a, far_b);

        assert!(
            far_dist > near_dist,
            "disjoint content ({far_dist} bits) should be further apart than single-word edit ({near_dist} bits)"
        );
        // And absolute distance for totally unrelated content should be meaningfully large.
        assert!(
            far_dist >= 16,
            "unrelated content should accumulate at least 16 bits of distance, got {far_dist}"
        );
    }

    #[test]
    fn simhash_is_case_insensitive_and_punct_insensitive() {
        // Tokenization is lowercase + ASCII punctuation split; these three
        // inputs tokenize identically and must hash to the same value.
        let a = simhash_64("Hello, World!", "Foo. Bar?");
        let b = simhash_64("hello world", "foo bar");
        let c = simhash_64("HELLO   WORLD", "FOO!!!BAR");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }
}
