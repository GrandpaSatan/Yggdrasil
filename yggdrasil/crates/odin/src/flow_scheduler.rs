/// Background Flow Scheduler — Yggdrasil "Always-On" Dream Mode (Sprint 055).
///
/// Runs in a separate tokio task, monitoring for:
/// 1. **Idle triggers** — when no user requests arrive for `min_idle_secs`, runs idle-triggered flows
/// 2. **Cron triggers** — runs flows on a cron schedule
///
/// The scheduler yields immediately when a user request comes in (idle flows are lower priority).
/// Dream flows run sequentially to avoid overloading GPU resources.
///
/// ## Sprint 063 P2 — Full cron parser
///
/// Schedules are now parsed into two shapes:
/// - **Legacy simple interval** (`"every_Nm"`) — retained verbatim for
///   backward compat with Sprint 055 configs.
/// - **Standard cron** (`"M H DoM Mo DoW"`) — parsed via the `cron` crate.
///
/// The dispatcher dispatches on format: starts with `every_` → interval;
/// contains a space → cron; else `None` (warn-logged, flow disabled).
///
/// Cron firing uses absolute-time tracking: each flow holds a `last_fire`
/// timestamp (initialised to `now` at scheduler start). On each 30s tick we
/// ask the cron schedule for the next fire after `last_fire`; if that time
/// is `<= now`, we fire and advance `last_fire` to the exact scheduled
/// instant. This guarantees: (a) each scheduled instant fires at most once,
/// (b) DST fallbacks do not double-fire, (c) missed ticks (due to GC or
/// sleep) catch up on the next tick instead of being skipped silently.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use cron::Schedule as CronSchedule;
use tokio::sync::watch;

use ygg_domain::config::{FlowConfig, FlowTrigger};

use crate::state::AppState;

/// Parsed schedule, returned by `parse_schedule`.
///
/// Kept `pub(crate)` so the test suite can assert the discriminant, without
/// leaking scheduling internals into the public API. The `Interval` variant's
/// field is the parsed minute count — the dispatcher extracts it directly in
/// `parse_simple_interval` on the legacy path and does not re-destructure
/// the enum variant, so clippy's dead-code heuristic flags it; `#[allow]` is
/// preferable to `_` because the field MUST round-trip through test asserts
/// (`test_parse_schedule_dispatches_interval`).
#[derive(Debug)]
#[allow(dead_code)] // Interval's u64 is consumed via pattern match in tests only.
pub(crate) enum ScheduleKind {
    /// Legacy `"every_Nm"` form — fires every N minutes.
    Interval(u64),
    /// Full cron expression (5 or 6 fields accepted by the `cron` crate).
    Cron(CronSchedule),
}

/// Tracks the last time a user request was processed.
/// Updated by the chat handler, read by the scheduler.
#[derive(Clone)]
pub struct ActivityTracker {
    tx: watch::Sender<Instant>,
    rx: watch::Receiver<Instant>,
}

impl ActivityTracker {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(Instant::now());
        Self { tx, rx }
    }

    /// Called by chat_handler on every incoming request.
    pub fn mark_active(&self) {
        let _ = self.tx.send(Instant::now());
    }

    /// Returns how long since the last user activity.
    pub fn idle_duration(&self) -> Duration {
        self.rx.borrow().elapsed()
    }
}

/// Parse a cron expression string.
///
/// The `cron` crate accepts both 5-field (standard Unix cron) and 6-field
/// (with seconds) expressions. We accept either: if a 5-field expression
/// is supplied we prepend `"0 "` to force second=0, matching Unix semantics
/// where `"0 3 * * *"` means "at 03:00:00".
pub(crate) fn parse_cron_expression(s: &str) -> Option<CronSchedule> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let field_count = trimmed.split_whitespace().count();
    let normalised = match field_count {
        5 => format!("0 {trimmed}"),
        6 | 7 => trimmed.to_string(),
        _ => return None,
    };
    normalised.parse::<CronSchedule>().ok()
}

/// Parse a schedule string into an `Interval` or `Cron` kind.
///
/// Dispatch rules:
/// - Starts with `"every_"` AND ends with `'m'` → `Interval(N)`.
/// - Contains a space (so has multiple fields) → `Cron(schedule)`.
/// - Anything else → `None` (the dispatcher warn-logs and skips the flow).
pub(crate) fn parse_schedule(schedule: &str) -> Option<ScheduleKind> {
    let s = schedule.trim();
    if s.starts_with("every_") && s.ends_with('m') {
        return s[6..s.len() - 1].parse::<u64>().ok().map(ScheduleKind::Interval);
    }
    if s.contains(' ') {
        return parse_cron_expression(s).map(ScheduleKind::Cron);
    }
    None
}

/// Parse simple interval from cron-like schedule string.
/// Supports: "every_Nm" (e.g. "every_240m" = every 4 hours).
/// Retained from Sprint 055 for backward compat — the Sprint 063 `parse_schedule`
/// dispatcher now owns full parsing, but this helper is kept as the single
/// canonical extractor for interval strings.
fn parse_simple_interval(schedule: &str) -> Option<u64> {
    if schedule.starts_with("every_") && schedule.ends_with('m') {
        schedule[6..schedule.len() - 1].parse().ok()
    } else {
        None
    }
}

/// A cron flow with its parsed schedule and last-fire bookkeeping.
///
/// Kept inside the scheduler — not exposed — so the hot path avoids
/// re-parsing the schedule string every 30s tick. The `last_fire`
/// timestamp is advanced only when we actually fire, guaranteeing that
/// each scheduled instant fires exactly once even across tick-boundary
/// jitter and brief task suspensions.
struct ParsedCronFlow<'a> {
    flow: &'a FlowConfig,
    schedule: CronSchedule,
    /// Absolute time of the last fire (or scheduler start, for first tick).
    last_fire: DateTime<Local>,
}

/// Decide whether a cron flow should fire on this tick.
///
/// Returns `Some(scheduled_time)` when the next scheduled instant after
/// `last_fire` is `<= now`. The caller MUST advance `last_fire` to the
/// returned `scheduled_time` on fire, so the next tick asks for the
/// *next* future instant — preventing DST-fallback double-fires and
/// tick-jitter duplicate fires.
fn should_fire(
    schedule: &CronSchedule,
    last_fire: DateTime<Local>,
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    let next = schedule.after(&last_fire).next()?;
    if next <= now {
        Some(next)
    } else {
        None
    }
}

/// Spawn the background flow scheduler.
///
/// This task runs for the lifetime of the server. It checks every 30 seconds
/// for idle or cron conditions and executes matching flows.
pub fn spawn_scheduler(
    state: Arc<AppState>,
    activity: ActivityTracker,
    flows: Vec<FlowConfig>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let check_interval = Duration::from_secs(30);

        // Separate flows by trigger type
        let idle_flows: Vec<&FlowConfig> = flows
            .iter()
            .filter(|f| matches!(f.trigger, FlowTrigger::Idle { .. }))
            .collect();

        // Parse cron schedules once at startup. Flows with an unparseable
        // schedule are warn-logged and skipped entirely — they will not
        // fire until the config is fixed and the scheduler restarted.
        let startup = Local::now();
        let mut cron_state: HashMap<String, ParsedCronFlow<'_>> = HashMap::new();
        for f in &flows {
            if let FlowTrigger::Cron { schedule } = &f.trigger {
                match parse_schedule(schedule) {
                    Some(ScheduleKind::Cron(cs)) => {
                        cron_state.insert(
                            f.name.clone(),
                            ParsedCronFlow { flow: f, schedule: cs, last_fire: startup },
                        );
                    }
                    Some(ScheduleKind::Interval(_)) | None => {
                        // Interval schedules handled on the legacy path below.
                        // `None` also falls through here — the legacy branch
                        // will warn-log it if interval parsing also fails.
                    }
                }
            }
        }

        let cron_legacy: Vec<&FlowConfig> = flows
            .iter()
            .filter(|f| match &f.trigger {
                FlowTrigger::Cron { schedule } => !cron_state.contains_key(&f.name)
                    && parse_simple_interval(schedule).is_some(),
                _ => false,
            })
            .collect();

        // Warn-log anything that parsed as neither — these flows will never fire.
        for f in &flows {
            if let FlowTrigger::Cron { schedule } = &f.trigger
                && !cron_state.contains_key(&f.name)
                && parse_simple_interval(schedule).is_none()
            {
                tracing::warn!(
                    flow = %f.name,
                    schedule = %schedule,
                    "cron schedule is neither 'every_Nm' nor a valid cron expression — flow disabled"
                );
            }
        }

        if idle_flows.is_empty() && cron_state.is_empty() && cron_legacy.is_empty() {
            tracing::info!("no background flows configured — scheduler idle");
            return;
        }

        tracing::info!(
            idle = idle_flows.len(),
            cron = cron_state.len() + cron_legacy.len(),
            cron_expressions = cron_state.len(),
            cron_intervals = cron_legacy.len(),
            "background flow scheduler started"
        );

        let mut last_interval_check = Instant::now();

        loop {
            tokio::time::sleep(check_interval).await;

            // ── Idle flows ───────────────────────────────────
            let idle_secs = activity.idle_duration().as_secs();

            for flow in &idle_flows {
                if let FlowTrigger::Idle { min_idle_secs } = &flow.trigger {
                    if idle_secs >= *min_idle_secs {
                        // Double-check: still idle? (user may have sent a request during our check)
                        if activity.idle_duration().as_secs() >= *min_idle_secs {
                            tracing::info!(
                                flow = %flow.name,
                                idle_secs = idle_secs,
                                "triggering idle flow"
                            );

                            match state.flow_engine.execute(flow, "consolidate", None, Some(&state)).await {
                                Ok(result) => {
                                    tracing::info!(
                                        flow = %flow.name,
                                        ms = result.elapsed_ms,
                                        steps = result.step_timings.len(),
                                        "idle flow complete"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        flow = %flow.name,
                                        error = %e,
                                        "idle flow failed"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // ── Full cron flows (Sprint 063 P2) ──────────────
            let now = Local::now();
            for entry in cron_state.values_mut() {
                if let Some(scheduled) = should_fire(&entry.schedule, entry.last_fire, now) {
                    tracing::info!(
                        flow = %entry.flow.name,
                        scheduled = %scheduled,
                        "triggering cron flow"
                    );
                    crate::metrics::record_cron_fire(&entry.flow.name);
                    match state.flow_engine.execute(entry.flow, "scheduled", None, Some(&state)).await {
                        Ok(result) => {
                            tracing::info!(
                                flow = %entry.flow.name,
                                ms = result.elapsed_ms,
                                "cron flow complete"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                flow = %entry.flow.name,
                                error = %e,
                                "cron flow failed"
                            );
                        }
                    }
                    // Advance last_fire to the exact scheduled instant so
                    // the next upcoming() query skips past it.
                    entry.last_fire = scheduled;
                }
            }

            // ── Legacy "every_Nm" intervals ──────────────────
            let since_last = last_interval_check.elapsed();
            if since_last >= Duration::from_secs(60) {
                last_interval_check = Instant::now();

                for flow in &cron_legacy {
                    if let FlowTrigger::Cron { schedule } = &flow.trigger
                        && let Some(mins) = parse_simple_interval(schedule)
                    {
                        let now_mins = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            / 60;

                        if now_mins % mins == 0 {
                            tracing::info!(
                                flow = %flow.name,
                                schedule = %schedule,
                                "triggering interval flow"
                            );

                            match state.flow_engine.execute(flow, "scheduled", None, Some(&state)).await {
                                Ok(result) => {
                                    tracing::info!(
                                        flow = %flow.name,
                                        ms = result.elapsed_ms,
                                        "interval flow complete"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        flow = %flow.name,
                                        error = %e,
                                        "interval flow failed"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}
