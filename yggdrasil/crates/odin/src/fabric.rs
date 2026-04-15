/// Yggdrasil Shared Memory Fabric client — Sprint 069 Phase G.3.
///
/// Thin HTTP client for the fabric service on Hugin :11450. Provides
/// the hooks Odin's flow engine calls before each step (`enrich`) and
/// after each step (`publish`).
///
/// The fabric is a L3 semantic store: publishes texts keyed by
/// flow_id + step_n, and retrieves related prior steps via cosine
/// similarity on embeddings.
///
/// Gated by env var `YGG_FABRIC_ENABLED=1`. When disabled, all calls
/// are no-ops so flows behave exactly as they do today. When enabled
/// and the fabric is unreachable, all calls degrade silently (log a
/// warning) — flows NEVER fail because of the fabric.
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Fabric client. Cheap to clone (wraps an Arc<reqwest::Client>).
#[derive(Clone, Debug)]
pub struct FabricClient {
    http: reqwest::Client,
    base_url: String,
    enabled: bool,
}

impl FabricClient {
    /// Build a client from env. Reads `YGG_FABRIC_URL` (default
    /// `http://10.0.65.9:11450`) and `YGG_FABRIC_ENABLED` (default
    /// `0`; set to `1` to activate).
    pub fn from_env() -> Self {
        let enabled = std::env::var("YGG_FABRIC_ENABLED").ok().as_deref() == Some("1");
        let base_url = std::env::var("YGG_FABRIC_URL")
            .unwrap_or_else(|_| "http://10.0.65.9:11450".to_string());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(2_000))
            .build()
            .unwrap_or_default();
        Self { http, base_url, enabled }
    }

    pub fn enabled(&self) -> bool { self.enabled }

    /// Publish a step output to the fabric. Silent on failure.
    pub async fn publish(&self, flow_id: &str, step_n: u32, model: &str, text: &str) {
        if !self.enabled || text.is_empty() { return; }
        let url = format!("{}/fabric/publish", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "flow_id": flow_id,
            "step_n": step_n,
            "model": model,
            "text": text,
        });
        match self.http.post(url).json(&body).send().await {
            Ok(r) if r.status().is_success() => {
                tracing::debug!(flow_id, step_n, model, "fabric.publish ok");
            }
            Ok(r) => {
                tracing::warn!(flow_id, step_n, status = %r.status(), "fabric.publish non-2xx");
            }
            Err(e) => {
                tracing::warn!(flow_id, err = %e, "fabric.publish transport error");
            }
        }
    }

    /// Query the fabric for prior steps in this flow. Returns at
    /// most `top_k` hits sorted by cosine similarity. Silent on
    /// failure (returns empty Vec).
    pub async fn query(&self, flow_id: &str, query_text: &str, top_k: usize) -> Vec<FabricHit> {
        if !self.enabled || query_text.is_empty() { return Vec::new(); }
        let url = format!("{}/fabric/query", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "flow_id": flow_id,
            "query_text": query_text,
            "top_k": top_k,
        });
        let resp = match self.http.post(url).json(&body).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(flow_id, status = %r.status(), "fabric.query non-2xx");
                return Vec::new();
            }
            Err(e) => {
                tracing::warn!(flow_id, err = %e, "fabric.query transport error");
                return Vec::new();
            }
        };
        match resp.json::<QueryResponse>().await {
            Ok(v) => v.hits,
            Err(e) => {
                tracing::warn!(err = %e, "fabric.query json decode");
                Vec::new()
            }
        }
    }

    /// Mark a flow as done — fabric evicts its records. Silent on failure.
    pub async fn done(&self, flow_id: &str) {
        if !self.enabled { return; }
        let url = format!("{}/fabric/done", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({ "flow_id": flow_id });
        let _ = self.http.post(url).json(&body).send().await;
    }
}

/// A single hit returned by `/fabric/query`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FabricHit {
    pub step_n: u32,
    pub model: String,
    pub text: String,
    pub similarity: f32,
    pub ts: i64,
}

#[derive(Deserialize)]
struct QueryResponse {
    #[serde(default)]
    hits: Vec<FabricHit>,
}

/// Format hits as a system-prompt prefix block. Called by the flow
/// engine before each step when YGG_FABRIC_ENABLED=1.
///
/// Returns an empty string if `hits` is empty so callers can
/// unconditionally prepend it.
pub fn format_working_memory_prefix(hits: &[FabricHit]) -> String {
    if hits.is_empty() { return String::new(); }
    let mut s = String::from("<working_memory>\n");
    for h in hits {
        // Cap each excerpt to keep prompt growth bounded.
        let excerpt: String = h.text.chars().take(600).collect();
        s.push_str(&format!("[step_{} {}]: {}\n", h.step_n, h.model, excerpt));
    }
    s.push_str("</working_memory>\n\n");
    s
}
