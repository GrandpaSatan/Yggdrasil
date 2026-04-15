//! Sprint 064 P7 — Vault-aware flow secrets.
//!
//! At flow dispatch (or step invocation), Odin resolves the union of
//! `FlowConfig.secrets` and `FlowStep.secrets` against the Mimir vault, then
//! substitutes `{{secret:<env_var>}}` tokens in any prompt/template string
//! with the resolved plaintext.
//!
//! Design choice: substitution-by-template (not process env vars). Setting
//! `std::env` in async/concurrent code is racy and would leak secrets across
//! requests. Template substitution gives the same UX (the flow author writes
//! `{{secret:HA_TOKEN}}`) without the global-state hazard.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ygg_domain::config::SecretRef;

const VAULT_FETCH_TIMEOUT_MS: u64 = 5000;

/// Errors the caller should surface to the user — secrets unavailable means
/// the flow cannot run safely (the prompt would still contain the literal
/// `{{secret:...}}` token, which would either confuse the model or leak the
/// reference upstream).
#[derive(Debug, thiserror::Error)]
pub enum FlowSecretsError {
    #[error("vault fetch failed for key '{key}' scope '{scope}': {reason}")]
    VaultFetch {
        key: String,
        scope: String,
        reason: String,
    },
    #[error("vault returned non-success for key '{key}' scope '{scope}': HTTP {status}")]
    VaultStatus {
        key: String,
        scope: String,
        status: u16,
    },
    #[error("vault response missing 'value' for key '{key}'")]
    VaultMissingValue { key: String },
}

/// Resolve the merged secret set for a flow + step into a `env_var → value`
/// map ready for prompt substitution. Step-level entries override flow-level
/// when they share an `env_var` name.
pub async fn resolve(
    client: &reqwest::Client,
    mimir_url: &str,
    vault_token: Option<&str>,
    flow_secrets: &[SecretRef],
    step_secrets: &[SecretRef],
) -> Result<HashMap<String, String>, FlowSecretsError> {
    let mut by_env: HashMap<String, &SecretRef> = HashMap::new();
    for s in flow_secrets {
        by_env.insert(s.env_var.clone(), s);
    }
    // Step-level overrides flow-level on conflict.
    for s in step_secrets {
        by_env.insert(s.env_var.clone(), s);
    }

    let mut out = HashMap::new();
    for (env_var, sref) in by_env {
        let value = fetch_one(client, mimir_url, vault_token, &sref.vault_key, &sref.scope).await?;
        out.insert(env_var, value);
    }
    Ok(out)
}

/// Substitute every `{{secret:NAME}}` token in `input` with the matching
/// entry from `secrets`. Tokens whose name is not in the map are left
/// in place verbatim so callers can detect the miss.
pub fn substitute(input: &str, secrets: &HashMap<String, String>) -> String {
    if secrets.is_empty() || !input.contains("{{secret:") {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("{{secret:") {
        out.push_str(&rest[..start]);
        let after_tag = &rest[start + "{{secret:".len()..];
        if let Some(end) = after_tag.find("}}") {
            let name = &after_tag[..end];
            if let Some(val) = secrets.get(name) {
                out.push_str(val);
            } else {
                // Unknown name — leave token verbatim.
                out.push_str("{{secret:");
                out.push_str(name);
                out.push_str("}}");
            }
            rest = &after_tag[end + 2..];
        } else {
            // Unterminated token; copy the rest as-is and stop.
            out.push_str(&rest[start..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// FLAW-008 (Sprint 069 Phase C): scrub resolved secret values from an
/// outbound LLM response / transcript / request log before we write it
/// anywhere visible.
///
/// A secret's plaintext value can end up in:
///   • the LLM's response content (if the model echoes back the prompt)
///   • request transcripts persisted by `SessionStore::append_messages`
///   • the `request_log` jsonl output
///
/// This function walks the response string and replaces EVERY exact
/// occurrence of each resolved secret value with the token
/// `{{secret:NAME redacted}}`. The 16-char prefix hash lets operators
/// trace which secret fired without leaking the value itself.
///
/// Called from `chat_handler` after streaming finishes and from
/// `SessionStore::append_messages` so both live transcripts AND the
/// on-disk request log are scrubbed.
pub fn scrub_response(response: &str, resolved_secrets: &HashMap<String, String>) -> String {
    if resolved_secrets.is_empty() || response.is_empty() {
        return response.to_owned();
    }
    // Sort longest-first to avoid partial-overlap collisions when one
    // secret is a substring of another.
    let mut pairs: Vec<(&String, &String)> = resolved_secrets.iter().collect();
    pairs.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));

    let mut out = response.to_owned();
    for (name, value) in pairs {
        if value.is_empty() || value.len() < 4 {
            // Too-short values would false-positive on common tokens.
            // Refuse to scrub them — caller should reject short secrets upstream.
            continue;
        }
        // Precompute a short fingerprint for telemetry without leaking the value.
        let fingerprint = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            value.as_bytes().hash(&mut h);
            format!("{:x}", h.finish())
        };
        let replacement = format!("{{{{secret:{name} redacted {}}}}}", &fingerprint[..8]);
        if out.contains(value) {
            out = out.replace(value, &replacement);
            tracing::debug!(
                secret = name,
                fp = %&fingerprint[..8],
                "scrubbed secret value from LLM response (FLAW-008)"
            );
        }
    }
    out
}

#[derive(Debug, Serialize)]
struct VaultRequest<'a> {
    action: &'a str,
    key: &'a str,
    scope: &'a str,
}

#[derive(Debug, Deserialize)]
struct VaultResponse {
    #[serde(default)]
    value: Option<String>,
}

async fn fetch_one(
    client: &reqwest::Client,
    mimir_url: &str,
    vault_token: Option<&str>,
    key: &str,
    scope: &str,
) -> Result<String, FlowSecretsError> {
    let url = format!("{}/api/v1/vault", mimir_url.trim_end_matches('/'));
    let body = VaultRequest {
        action: "get",
        key,
        scope,
    };
    let mut req = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_millis(VAULT_FETCH_TIMEOUT_MS));
    if let Some(token) = vault_token {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| FlowSecretsError::VaultFetch {
            key: key.to_owned(),
            scope: scope.to_owned(),
            reason: e.to_string(),
        })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(FlowSecretsError::VaultStatus {
            key: key.to_owned(),
            scope: scope.to_owned(),
            status: status.as_u16(),
        });
    }

    let parsed: VaultResponse = resp.json().await.map_err(|e| FlowSecretsError::VaultFetch {
        key: key.to_owned(),
        scope: scope.to_owned(),
        reason: format!("parse: {e}"),
    })?;

    parsed
        .value
        .ok_or_else(|| FlowSecretsError::VaultMissingValue {
            key: key.to_owned(),
        })
}
