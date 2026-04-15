/// Static registry of MCP tools available to the agent loop.
///
/// Each tool has a name, description, JSON Schema for parameters, a safety tier,
/// and an endpoint describing how to execute it via HTTP.  The registry is built
/// once at startup and shared via `AppState`.
///
/// Tool metadata (name, description, tier, keywords) comes from the canonical
/// catalog in `ygg_domain::tools`.  This module adds Odin-specific endpoint
/// routing and JSON parameter schemas.
use std::time::Duration;

use serde_json::{json, Value as JsonValue};
use ygg_domain::tool_params::schema_for_tool;
use ygg_domain::tools as catalog;

use crate::openai::{FunctionDefinition, ToolDefinition};
use crate::state::AppState;

// ─────────────────────────────────────────────────────────────────
// Tier & endpoint types
// ─────────────────────────────────────────────────────────────────

/// Safety tier controlling which tools an LLM agent may call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTier {
    /// Read-only, always allowed.
    Safe,
    /// Write operations, require explicit opt-in.
    Restricted,
    /// Never allowed for LLM agents (device control, filesystem writes).
    Blocked,
}

/// Convert from the canonical catalog tier to Odin's local tier type.
fn convert_tier(t: catalog::ToolTier) -> ToolTier {
    match t {
        catalog::ToolTier::Safe => ToolTier::Safe,
        catalog::ToolTier::Restricted => ToolTier::Restricted,
        catalog::ToolTier::Blocked => ToolTier::Blocked,
    }
}

/// Build a `ToolSpec` by pulling metadata + parameter schema from the canonical catalog.
///
/// Only the endpoint routing is Odin-specific; everything else (name,
/// description, tier, keywords, timeout, voice_always, parameter schema)
/// comes from `ygg_domain`.
fn from_catalog(name: &str, endpoint: ToolEndpoint) -> ToolSpec {
    let meta = catalog::find_meta(name)
        .unwrap_or_else(|| panic!("tool '{name}' not found in ygg_domain::tools catalog"));
    let parameters_schema = schema_for_tool(name)
        .unwrap_or_else(|| panic!("tool '{name}' has no schema in ygg_domain::tool_params"));
    ToolSpec {
        name: meta.name,
        description: meta.description,
        parameters_schema,
        tier: convert_tier(meta.tier),
        endpoint,
        timeout_override_secs: meta.timeout_override_secs,
        keywords: meta.keywords,
        voice_always: meta.voice_always,
    }
}

/// Where a tool's HTTP request should be sent.
#[derive(Debug, Clone)]
pub enum ToolEndpoint {
    /// Mimir memory service — uses `state.mimir_url`.
    Mimir(&'static str),
    /// Muninn code search — uses `state.muninn_url`.
    Muninn(&'static str),
    /// Odin's own HTTP routes (e.g. /v1/models, /health).
    OdinSelf(&'static str),
    /// Home Assistant via the HA client in AppState.
    Ha(HaToolKind),
}

/// Sub-types for Home Assistant tool dispatch.
#[derive(Debug, Clone)]
pub enum HaToolKind {
    GetStates,
    ListEntities,
    CallService,
    GenerateAutomation,
}

// ─────────────────────────────────────────────────────────────────
// Tool spec
// ─────────────────────────────────────────────────────────────────

/// A tool available to the agent loop.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters_schema: JsonValue,
    pub tier: ToolTier,
    pub endpoint: ToolEndpoint,
    /// Optional per-tool timeout override (seconds). When `Some`, overrides
    /// the global `AgentLoopConfig.tool_timeout_secs` for this tool only.
    /// Used for long-running operations like gaming VM launches (WOL + boot).
    pub timeout_override_secs: Option<u64>,
    /// Keyword triggers for voice query-based tool selection.
    /// When the user's voice query contains any of these substrings (case-insensitive),
    /// this tool is included in the agent loop context.
    pub keywords: &'static [&'static str],
    /// Core tool — always included in keyword-based selection regardless of query.
    pub voice_always: bool,
}

// ─────────────────────────────────────────────────────────────────
// Registry builder
// ─────────────────────────────────────────────────────────────────

/// Build the complete tool registry.  Called once at startup.
///
/// Tool metadata (name, description, tier, keywords, timeout, voice_always) is
/// pulled from the canonical catalog in `ygg_domain::tools::ALL_TOOLS`.
/// Only the endpoint routing and JSON parameter schemas are Odin-specific.
pub fn build_registry() -> Vec<ToolSpec> {
    vec![
        // ── Safe tier (read-only) ───────────────────────────────
        from_catalog("search_code",        ToolEndpoint::Muninn("/api/v1/search")),
        from_catalog("query_memory",       ToolEndpoint::Mimir("/api/v1/query")),
        from_catalog("memory_intersect",   ToolEndpoint::Mimir("/api/v1/sdr/operations")),
        from_catalog("get_sprint_history", ToolEndpoint::Mimir("/api/v1/sprints/list")),
        from_catalog("memory_timeline",    ToolEndpoint::Mimir("/api/v1/timeline")),
        from_catalog("list_models",        ToolEndpoint::OdinSelf("/v1/models")),
        from_catalog("service_health",     ToolEndpoint::OdinSelf("/health")),
        from_catalog("ast_analyze",        ToolEndpoint::Muninn("/api/v1/symbols")),
        from_catalog("impact_analysis",    ToolEndpoint::Muninn("/api/v1/references")),
        from_catalog("ha_get_states",      ToolEndpoint::Ha(HaToolKind::GetStates)),
        from_catalog("ha_list_entities",   ToolEndpoint::Ha(HaToolKind::ListEntities)),
        from_catalog("config_version",     ToolEndpoint::OdinSelf("/api/v1/version")),
        from_catalog("web_search",         ToolEndpoint::OdinSelf("/api/v1/web_search")),
        from_catalog("network_topology",   ToolEndpoint::OdinSelf("/api/v1/mesh/nodes")),

        // ── Restricted tier (write operations) ──────────────────
        from_catalog("ha_call_service",        ToolEndpoint::Ha(HaToolKind::CallService)),
        from_catalog("ha_generate_automation", ToolEndpoint::Ha(HaToolKind::GenerateAutomation)),
        from_catalog("gaming",             ToolEndpoint::OdinSelf("/api/v1/gaming")),
        from_catalog("store_memory",       ToolEndpoint::Mimir("/api/v1/store")),
        from_catalog("context_offload",    ToolEndpoint::Mimir("/api/v1/context")),
        from_catalog("context_bridge",     ToolEndpoint::Mimir("/api/v1/store")),
        from_catalog("task_queue",         ToolEndpoint::Mimir("/api/v1/tasks")),
        from_catalog("memory_graph",       ToolEndpoint::Mimir("/api/v1/graph")),
        from_catalog("vault",              ToolEndpoint::Mimir("/api/v1/vault")),
        from_catalog("config_sync",        ToolEndpoint::OdinSelf("/api/v1/version")),
        from_catalog("build_check",        ToolEndpoint::OdinSelf("/api/v1/build_check")),
        from_catalog("deploy",             ToolEndpoint::OdinSelf("/api/v1/deploy")),
        from_catalog("generate",           ToolEndpoint::OdinSelf("/v1/chat/completions")),
        from_catalog("delegate",           ToolEndpoint::OdinSelf("/v1/chat/completions")),
        from_catalog("task_delegate",      ToolEndpoint::OdinSelf("/v1/chat/completions")),
        from_catalog("diff_review",        ToolEndpoint::OdinSelf("/v1/chat/completions")),

        // ── Research flow tools (Sprint 056) ───────────────────
        from_catalog("search_documents",   ToolEndpoint::Muninn("/api/v1/documents/search")),
        from_catalog("ingest_document",    ToolEndpoint::Mimir("/api/v1/documents/ingest")),
        from_catalog("research_report",    ToolEndpoint::Mimir("/api/v1/store")),
    ]
}

// ─────────────────────────────────────────────────────────────────
// Conversion to OpenAI tool definitions
// ─────────────────────────────────────────────────────────────────

/// Filter tools by allowed tiers and convert to OpenAI `ToolDefinition` format.
pub fn to_tool_definitions(specs: &[ToolSpec], allowed_tiers: &[ToolTier]) -> Vec<ToolDefinition> {
    specs
        .iter()
        .filter(|s| allowed_tiers.contains(&s.tier))
        .map(|s| ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: s.name.to_string(),
                description: s.description.to_string(),
                parameters: s.parameters_schema.clone(),
            },
        })
        .collect()
}

/// Filter tools by allowed tiers AND a name allowlist.
pub fn to_tool_definitions_filtered(
    specs: &[ToolSpec],
    allowed_tiers: &[ToolTier],
    allowed_names: &[String],
) -> Vec<ToolDefinition> {
    specs
        .iter()
        .filter(|s| allowed_tiers.contains(&s.tier) && allowed_names.iter().any(|n| n == s.name))
        .map(|s| ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: s.name.to_string(),
                description: s.description.to_string(),
                parameters: s.parameters_schema.clone(),
            },
        })
        .collect()
}

/// Select tools for a voice query using keyword matching.
///
/// Returns tools whose `keywords` match substrings in the query (case-insensitive),
/// plus any tools marked `voice_always`. Falls back to only `voice_always` tools
/// when no keywords match.
pub fn select_tools_for_query(
    specs: &[ToolSpec],
    query: &str,
    allowed_tiers: &[ToolTier],
) -> Vec<ToolDefinition> {
    let query_lower = query.to_lowercase();
    specs
        .iter()
        .filter(|s| {
            allowed_tiers.contains(&s.tier)
                && (s.voice_always
                    || s.keywords.iter().any(|kw| query_lower.contains(kw)))
        })
        .map(|s| ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: s.name.to_string(),
                description: s.description.to_string(),
                parameters: s.parameters_schema.clone(),
            },
        })
        .collect()
}

/// Look up a tool spec by name.
pub fn find_tool<'a>(registry: &'a [ToolSpec], name: &str) -> Option<&'a ToolSpec> {
    registry.iter().find(|s| s.name == name)
}

/// Check whether a tool name is allowed given the tier filter.
pub fn is_tool_allowed(registry: &[ToolSpec], name: &str, allowed_tiers: &[ToolTier]) -> bool {
    registry
        .iter()
        .any(|s| s.name == name && allowed_tiers.contains(&s.tier))
}

// ─────────────────────────────────────────────────────────────────
// Tool execution (HTTP dispatch)
// ─────────────────────────────────────────────────────────────────

/// Execute a tool call by dispatching to the appropriate backend service.
///
/// Returns the response body as a string (success) or an error message (failure).
/// The LLM sees both — it can interpret errors and decide to retry or give up.
///
/// Mimir and Muninn endpoints are protected by a circuit breaker: after 3
/// consecutive failures the endpoint is short-circuited for 30 seconds.
pub async fn execute_tool(
    state: &AppState,
    spec: &ToolSpec,
    arguments: &JsonValue,
    timeout: Duration,
) -> Result<String, String> {
    // Resolve the base URL for circuit breaker tracking.
    let (base_url, use_breaker) = match &spec.endpoint {
        ToolEndpoint::Mimir(_) => (Some(state.mimir_url.as_str()), true),
        ToolEndpoint::Muninn(_) => (Some(state.muninn_url.as_str()), true),
        _ => (None, false),
    };

    // Check circuit breaker before dispatching.
    let breaker = if use_breaker {
        let b = state.circuit_breakers.get(base_url.unwrap());
        if !b.allow_request() {
            return Err(format!(
                "Service at {} is temporarily unavailable (circuit breaker open). \
                 Try a different approach or skip this tool.",
                base_url.unwrap()
            ));
        }
        Some(b)
    } else {
        None
    };

    let result = match &spec.endpoint {
        ToolEndpoint::Mimir(path) => {
            let url = format!("{}{}", state.mimir_url, path);
            http_post(&state.http_client, &url, arguments, timeout).await
        }
        ToolEndpoint::Muninn(path) => {
            let url = format!("{}{}", state.muninn_url, path);
            http_post(&state.http_client, &url, arguments, timeout).await
        }
        ToolEndpoint::OdinSelf(path) => {
            // Fast-path: handle trivial OdinSelf endpoints directly to avoid
            // HTTP loopback overhead. Complex handlers still use HTTP.
            match *path {
                "/api/v1/version" => {
                    Ok(json!({ "version": env!("CARGO_PKG_VERSION") }).to_string())
                }
                _ => {
                    // Fall back to HTTP loopback for complex handlers
                    // (web_search, gaming, list_models, health).
                    let url = format!("http://{}{}", state.config.listen_addr, path);
                    if arguments.as_object().is_some_and(|o| o.is_empty()) || arguments.is_null() {
                        http_get(&state.http_client, &url, timeout).await
                    } else {
                        http_post(&state.http_client, &url, arguments, timeout).await
                    }
                }
            }
        }
        ToolEndpoint::Ha(kind) => execute_ha_tool(state, kind, arguments).await,
    };

    // Update circuit breaker state based on result.
    if let Some(b) = breaker {
        match &result {
            Ok(_) => b.record_success(),
            Err(_) => b.record_failure(),
        }
    }

    result
}

/// Whether a failed HTTP response warrants a retry.
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::SERVICE_UNAVAILABLE       // 503
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS  // 429
        || status == reqwest::StatusCode::GATEWAY_TIMEOUT    // 504
}

/// Whether a reqwest send error is transient (connection refused, reset, DNS).
fn is_retryable_error(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout()
}

/// Retry backoff base delays: 200ms, then 800ms.
/// Actual delay is jittered to 50%–150% of base to prevent thundering herd.
const RETRY_DELAYS_MS: [u64; 2] = [200, 800];

/// Compute a jittered delay from a base value (50%–150% of base).
fn jittered_delay(base_ms: u64) -> Duration {
    use rand::Rng;
    let jitter = rand::thread_rng().gen_range(0.5..1.5);
    Duration::from_millis((base_ms as f64 * jitter) as u64)
}

async fn http_post(
    client: &reqwest::Client,
    url: &str,
    body: &JsonValue,
    timeout: Duration,
) -> Result<String, String> {
    let mut last_err = String::new();

    for attempt in 0..=RETRY_DELAYS_MS.len() {
        let result = client
            .post(url)
            .json(body)
            .timeout(timeout)
            .send()
            .await;

        match result {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.map_err(|e| format!("Failed to read response: {e}"))?;
                if status.is_success() {
                    return Ok(text);
                }
                if attempt < RETRY_DELAYS_MS.len() && is_retryable_status(status) {
                    tracing::debug!(url, attempt, %status, "retryable HTTP status, backing off");
                    tokio::time::sleep(jittered_delay(RETRY_DELAYS_MS[attempt])).await;
                    last_err = format!("HTTP {status}: {text}");
                    continue;
                }
                return Err(format!("HTTP {status}: {text}"));
            }
            Err(e) => {
                if attempt < RETRY_DELAYS_MS.len() && is_retryable_error(&e) {
                    tracing::debug!(url, attempt, error = %e, "retryable connection error, backing off");
                    tokio::time::sleep(jittered_delay(RETRY_DELAYS_MS[attempt])).await;
                    last_err = format!("HTTP request failed: {e}");
                    continue;
                }
                return Err(format!("HTTP request failed: {e}"));
            }
        }
    }

    Err(last_err)
}

async fn http_get(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<String, String> {
    let mut last_err = String::new();

    for attempt in 0..=RETRY_DELAYS_MS.len() {
        let result = client
            .get(url)
            .timeout(timeout)
            .send()
            .await;

        match result {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.map_err(|e| format!("Failed to read response: {e}"))?;
                if status.is_success() {
                    return Ok(text);
                }
                if attempt < RETRY_DELAYS_MS.len() && is_retryable_status(status) {
                    tracing::debug!(url, attempt, %status, "retryable HTTP status, backing off");
                    tokio::time::sleep(jittered_delay(RETRY_DELAYS_MS[attempt])).await;
                    last_err = format!("HTTP {status}: {text}");
                    continue;
                }
                return Err(format!("HTTP {status}: {text}"));
            }
            Err(e) => {
                if attempt < RETRY_DELAYS_MS.len() && is_retryable_error(&e) {
                    tracing::debug!(url, attempt, error = %e, "retryable connection error, backing off");
                    tokio::time::sleep(jittered_delay(RETRY_DELAYS_MS[attempt])).await;
                    last_err = format!("HTTP request failed: {e}");
                    continue;
                }
                return Err(format!("HTTP request failed: {e}"));
            }
        }
    }

    Err(last_err)
}

async fn execute_ha_tool(
    state: &AppState,
    kind: &HaToolKind,
    arguments: &JsonValue,
) -> Result<String, String> {
    let ha = state
        .ha_client
        .as_ref()
        .ok_or_else(|| "Home Assistant is not configured".to_string())?;

    match kind {
        HaToolKind::GetStates => {
            let entity_id = arguments.get("entity_id").and_then(|v| v.as_str());
            let domain = arguments.get("domain").and_then(|v| v.as_str());
            let states = ha.get_states().await.map_err(|e| format!("HA error: {e}"))?;

            if let Some(eid) = entity_id {
                // Specific entity → full details
                let filtered: Vec<_> = states.iter().filter(|s| s.entity_id == eid).collect();
                serde_json::to_string_pretty(&filtered).map_err(|e| format!("JSON error: {e}"))
            } else if let Some(d) = domain {
                // Domain filter → full details for matching entities
                let prefix = format!("{d}.");
                let filtered: Vec<_> = states.iter().filter(|s| s.entity_id.starts_with(&prefix)).collect();
                serde_json::to_string_pretty(&filtered).map_err(|e| format!("JSON error: {e}"))
            } else {
                // No filter → compact summary (entity_id + state only) to avoid
                // blowing the agent loop's tool output truncation limit.
                let compact: Vec<_> = states
                    .iter()
                    .map(|s| json!({ "entity_id": s.entity_id, "state": s.state }))
                    .collect();
                serde_json::to_string_pretty(&compact).map_err(|e| format!("JSON error: {e}"))
            }
        }
        HaToolKind::ListEntities => {
            let domain = arguments.get("domain").and_then(|v| v.as_str());
            let states = ha.get_states().await.map_err(|e| format!("HA error: {e}"))?;
            let entities: Vec<&str> = states
                .iter()
                .filter(|s| {
                    domain
                        .map(|d| s.entity_id.starts_with(&format!("{d}.")))
                        .unwrap_or(true)
                })
                .map(|s| s.entity_id.as_str())
                .collect();
            serde_json::to_string_pretty(&entities).map_err(|e| format!("JSON error: {e}"))
        }
        HaToolKind::CallService => {
            let domain = arguments
                .get("domain")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing required field 'domain'".to_string())?;
            let service = arguments
                .get("service")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing required field 'service'".to_string())?;
            let data = arguments
                .get("data")
                .cloned()
                .unwrap_or(json!({}));

            const ALLOWED_DOMAINS: &[&str] = &[
                "light", "switch", "cover", "fan", "media_player", "scene",
                "script", "input_boolean", "input_number", "input_select",
                "input_text", "automation", "climate", "vacuum", "button",
                "number", "select", "humidifier", "water_heater",
            ];
            if !ALLOWED_DOMAINS.contains(&domain) {
                return Err(format!(
                    "Domain '{}' is not allowed. Allowed: {}",
                    domain,
                    ALLOWED_DOMAINS.join(", ")
                ));
            }

            ha.call_service(domain, service, data, ygg_ha::HaClient::DEFAULT_ALLOWED_DOMAINS)
                .await
                .map_err(|e| format!("HA call_service error: {e}"))?;

            Ok(format!("Successfully called {domain}.{service}"))
        }
        HaToolKind::GenerateAutomation => {
            let description = arguments
                .get("description")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing required field 'description'".to_string())?;

            // AutomationGenerator needs both the HA client and the LLM.
            // Create a generator that routes through Odin for LLM calls.
            let generator = ygg_ha::AutomationGenerator::new(
                &format!("http://{}", state.config.listen_addr),
                &state.config.routing.default_model,
            );

            generator
                .generate_automation(ha, description)
                .await
                .map(|yaml| format!("## Generated Automation\n\n```yaml\n{yaml}\n```"))
                .map_err(|e| format!("HA automation generation error: {e}"))
        }
    }
}
