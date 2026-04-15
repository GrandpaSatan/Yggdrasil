use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::HaError;
use ygg_domain::config::HaConfig;

/// REST API client for Home Assistant.
#[derive(Clone)]
pub struct HaClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

/// An HA entity state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: serde_json::Value,
    #[serde(default)]
    pub last_changed: Option<String>,
}

/// Services available within a single HA domain (e.g., `light`, `switch`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainServices {
    pub domain: String,
    pub services: HashMap<String, ServiceDef>,
}

/// Definition of a single HA service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fields: HashMap<String, serde_json::Value>,
}

impl HaClient {
    pub fn from_config(config: &HaConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            base_url: config.url.trim_end_matches('/').to_string(),
            token: config.token.clone(),
        }
    }

    /// Get all entity states.
    pub async fn get_states(&self) -> Result<Vec<EntityState>, HaError> {
        let url = format!("{}/api/states", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| HaError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(HaError::Api(format!("{status}: {body}")));
        }

        resp.json()
            .await
            .map_err(|e| HaError::Parse(e.to_string()))
    }

    /// Get a specific entity state.
    pub async fn get_entity(&self, entity_id: &str) -> Result<EntityState, HaError> {
        let url = format!("{}/api/states/{entity_id}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| HaError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(HaError::Api(format!("{status}: {body}")));
        }

        resp.json()
            .await
            .map_err(|e| HaError::Parse(e.to_string()))
    }

    /// List entity states, optionally filtered to a single HA domain.
    ///
    /// If `domain` is `Some("light")`, returns only entities whose `entity_id`
    /// starts with `"light."`.  If `None`, returns all entities (equivalent to
    /// `get_states()`).
    pub async fn list_entities(
        &self,
        domain: Option<&str>,
    ) -> Result<Vec<EntityState>, HaError> {
        let all = self.get_states().await?;
        match domain {
            None => Ok(all),
            Some(d) => {
                let prefix = format!("{d}.");
                Ok(all
                    .into_iter()
                    .filter(|e| e.entity_id.starts_with(&prefix))
                    .collect())
            }
        }
    }

    /// Get all services available on the HA instance.
    ///
    /// Calls `GET /api/services`.  The HA response is an array of objects,
    /// each with a `domain` key and a `services` object.
    pub async fn get_services(&self) -> Result<Vec<DomainServices>, HaError> {
        let url = format!("{}/api/services", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| HaError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(HaError::Api(format!("{status}: {body}")));
        }

        resp.json()
            .await
            .map_err(|e| HaError::Parse(e.to_string()))
    }

    /// Default HA domain allowlist (Sprint 069 Phase C, VULN-005).
    ///
    /// These are the only domains the default call_service path will dispatch.
    /// Callers can supply their own allowlist for privileged contexts.
    pub const DEFAULT_ALLOWED_DOMAINS: &'static [&'static str] = &[
        "light",
        "switch",
        "media_player",
        "script",
        "automation",
        "scene",
        "cover",
        "climate",
        "fan",
        "notify",
        "persistent_notification",
        "homeassistant",
        "input_boolean",
        "input_button",
    ];

    /// Call an HA service (e.g., turn on a light).
    ///
    /// Sprint 069 Phase C (VULN-005): `allowed_domains` restricts the set of
    /// domains this call will dispatch to. Callers supply either
    /// `HaClient::DEFAULT_ALLOWED_DOMAINS` for the stock permit-list or their
    /// own curated slice for privileged code paths. A mismatched domain
    /// returns `HaError::DomainNotAllowed` before any HTTP is sent.
    pub async fn call_service(
        &self,
        domain: &str,
        service: &str,
        data: serde_json::Value,
        allowed_domains: &[&str],
    ) -> Result<(), HaError> {
        if !allowed_domains.iter().any(|d| d.eq_ignore_ascii_case(domain)) {
            return Err(HaError::DomainNotAllowed {
                domain: domain.to_string(),
            });
        }
        let url = format!("{}/api/services/{domain}/{service}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&data)
            .send()
            .await
            .map_err(|e| HaError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(HaError::Api(format!("{status}: {body}")));
        }
        Ok(())
    }
}
