//! ygg-dreamer binary entrypoint.
//!
//! Spawns three long-lived tokio tasks:
//! 1. `health_server` — axum HTTP server on `listen_addr` for `/health` + `/metrics`.
//! 2. `activity_poller` — polls Odin `/internal/activity` every `poll_interval_secs`.
//! 3. `warmup_loop` — when idle_duration > min_idle_secs, fires each configured
//!    warmup prefix sequentially (spaced by `warmup_interval_secs`).
//!
//! Sprint 065 C·P5 — systemd integration via `deploy/systemd/yggdrasil-dreamer.service`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{Json, Router, routing::get};
use clap::Parser;
use reqwest::Client;
use serde_json::json;

use ygg_dreamer::config::DreamerConfig;
use ygg_dreamer::{flow_runner, warmup};

#[derive(Parser, Debug)]
#[command(name = "ygg-dreamer", about = "Yggdrasil dream-mode daemon")]
struct Cli {
    #[arg(long, default_value = "/opt/yggdrasil/config/dreamer.config.json")]
    config: PathBuf,
}

#[derive(Default)]
struct DreamerState {
    idle_secs: AtomicU64,
    warmup_fires: AtomicU64,
    dream_fires: AtomicU64,
    /// Sprint 068 Phase 6b: UNIX seconds of the most recent warmup/dream fire.
    /// 0 when the daemon has not yet fired anything.
    last_fire_ts: AtomicU64,
    /// Sprint 068 Phase 6b: name of the dream flow currently being
    /// dispatched. Set before `flow_runner::run_dream` and cleared after.
    /// Whitelisted against `DreamerConfig.dream_flows[].name` at config
    /// load so `/status` never leaks internal warmup prefix names.
    active_flow: RwLock<Option<String>>,
}

/// Seconds since UNIX epoch — fallible-safe wrapper around SystemTime.
fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,ygg_dreamer=debug".into()),
        )
        .init();

    let cli = Cli::parse();
    let cfg = DreamerConfig::load(&cli.config)?;
    tracing::info!(
        config = %cli.config.display(),
        odin_url = %cfg.odin_url,
        min_idle_secs = cfg.min_idle_secs,
        warmup_prefixes = cfg.warmup_prefixes.len(),
        dream_flows = cfg.dream_flows.len(),
        "ygg-dreamer starting"
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()?;

    let state = Arc::new(DreamerState::default());

    // --- Health + status server ---
    // /health is the pre-068 smoke-test endpoint (minimal payload, stable
    // shape consumed by systemd + existing probes). /status is the
    // Sprint 068 Phase 6b richer payload consumed by the VS Code extension's
    // Models tree live-status poller.
    let health_state = state.clone();
    let status_state = state.clone();
    let listen_addr = cfg.listen_addr.clone();
    let health_handle = tokio::spawn(async move {
        let app = Router::new()
            .route(
                "/health",
                get(move || {
                    let s = health_state.clone();
                    async move {
                        Json(json!({
                            "status": "ok",
                            "service": "ygg-dreamer",
                            "idle_secs": s.idle_secs.load(Ordering::Relaxed),
                            "warmup_fires": s.warmup_fires.load(Ordering::Relaxed),
                            "dream_fires": s.dream_fires.load(Ordering::Relaxed),
                        }))
                    }
                }),
            )
            .route(
                "/status",
                get(move || {
                    let s = status_state.clone();
                    async move {
                        let last_fire_ts = s.last_fire_ts.load(Ordering::Relaxed);
                        let idle_secs = s.idle_secs.load(Ordering::Relaxed);
                        let now = unix_now_secs();
                        // `active` = a fire happened within the last 60s AND
                        // the daemon is currently inside an idle window
                        // (idle_secs < 10 means we're actively dispatching
                        // a dream flow rather than waiting for idle).
                        let active = last_fire_ts > 0
                            && now.saturating_sub(last_fire_ts) < 60
                            && idle_secs < 10;
                        let active_flow = s
                            .active_flow
                            .read()
                            .ok()
                            .and_then(|g| g.clone());
                        Json(json!({
                            "status": "ok",
                            "service": "ygg-dreamer",
                            "idle_secs": idle_secs,
                            "warmup_fires": s.warmup_fires.load(Ordering::Relaxed),
                            "dream_fires": s.dream_fires.load(Ordering::Relaxed),
                            "active": active,
                            "active_flow": active_flow,
                            "last_fire_ts": last_fire_ts,
                        }))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind(&listen_addr).await
            .expect("bind health server");
        tracing::info!(addr = %listen_addr, "dreamer health server listening");
        axum::serve(listener, app).await.ok();
    });

    // --- Activity poller + warmup loop (single task) ---
    let odin_url = cfg.odin_url.clone();
    let mimir_url = cfg.mimir_url.clone();
    let poll_interval = Duration::from_secs(cfg.poll_interval_secs);
    let warmup_interval = Duration::from_secs(cfg.warmup_interval_secs);
    let min_idle = cfg.min_idle_secs;
    let warmup_prefixes = cfg.warmup_prefixes.clone();
    let dream_flows = cfg.dream_flows.clone();
    let sprint_tag = cfg.sprint_tag.clone();
    let dream_client = client.clone();
    let dream_state = state.clone();

    let dream_handle = tokio::spawn(async move {
        let mut last_warmup = tokio::time::Instant::now() - warmup_interval;

        loop {
            tokio::time::sleep(poll_interval).await;

            // Poll Odin activity.
            let idle = match dream_client
                .get(format!("{}/internal/activity", odin_url.trim_end_matches('/')))
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<serde_json::Value>().await {
                        Ok(v) => v.get("idle_secs").and_then(|x| x.as_u64()).unwrap_or(0),
                        Err(e) => {
                            tracing::warn!(error = %e, "dreamer: activity body parse failed");
                            0
                        }
                    }
                }
                Ok(resp) => {
                    tracing::warn!(status = %resp.status(), "dreamer: activity non-success");
                    0
                }
                Err(e) => {
                    tracing::warn!(error = %e, "dreamer: activity fetch failed");
                    0
                }
            };
            dream_state.idle_secs.store(idle, Ordering::Relaxed);

            if idle < min_idle {
                continue;
            }

            // Warmup pass — gated by warmup_interval to avoid burning GPU.
            if last_warmup.elapsed() >= warmup_interval {
                for prefix in &warmup_prefixes {
                    match warmup::fire_one(&dream_client, prefix).await {
                        Ok(()) => {
                            tracing::info!(prefix = %prefix.name, "warmup fired");
                            dream_state.warmup_fires.fetch_add(1, Ordering::Relaxed);
                            // Sprint 068 Phase 6b: track recency for /status.
                            dream_state
                                .last_fire_ts
                                .store(unix_now_secs(), Ordering::Relaxed);
                        }
                        Err(e) => {
                            tracing::warn!(prefix = %prefix.name, error = %e, "warmup failed");
                        }
                    }
                }
                last_warmup = tokio::time::Instant::now();
            }

            // Dream flow pass — one per idle window, rotate through configured flows.
            for flow in &dream_flows {
                // Sprint 068 Phase 6b: publish the active flow name for
                // /status. The name comes from user-owned config
                // (DreamerConfig.dream_flows[].name), so no whitelist
                // check is needed — it's already user-facing.
                if let Ok(mut guard) = dream_state.active_flow.write() {
                    *guard = Some(flow.name.clone());
                }
                let result = flow_runner::run_dream(
                    &dream_client,
                    &odin_url,
                    &mimir_url,
                    flow,
                    &sprint_tag,
                )
                .await;
                // Clear regardless of success/failure so /status doesn't
                // get stuck reporting a stale active_flow after an error.
                if let Ok(mut guard) = dream_state.active_flow.write() {
                    *guard = None;
                }

                match result {
                    Ok(text) => {
                        tracing::info!(
                            flow = %flow.name,
                            chars = text.len(),
                            "dream flow completed, engram stored"
                        );
                        dream_state.dream_fires.fetch_add(1, Ordering::Relaxed);
                        dream_state
                            .last_fire_ts
                            .store(unix_now_secs(), Ordering::Relaxed);
                    }
                    Err(e) => {
                        tracing::warn!(flow = %flow.name, error = %e, "dream flow failed");
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = health_handle => tracing::warn!("dreamer: health task exited"),
        _ = dream_handle => tracing::warn!("dreamer: dream task exited"),
    }

    Ok(())
}
