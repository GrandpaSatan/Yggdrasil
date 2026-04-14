//! Unified configuration loader for Yggdrasil.
//!
//! Provides [`load_json`] for loading any serde-deserializable config from JSON
//! or YAML (auto-detected by file extension) with `${ENV_VAR}` placeholder
//! expansion, [`validate`] for config validation, and [`watch`] for hot-reload
//! via filesystem notifications.

mod expand;
pub mod impls;
pub mod validate;
pub mod watch;

use std::path::Path;

use serde::de::DeserializeOwned;

pub use expand::expand_env_vars;
pub use validate::{Validate, ValidationError};
pub use watch::ConfigWatcher;

// Re-export domain config types for convenience.
pub use ygg_domain::config;

/// Errors returned by config loading functions.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file '{path}': {source}")]
    ReadFile {
        path: String,
        source: std::io::Error,
    },

    #[error("failed to parse JSON config '{path}': {source}")]
    ParseJson {
        path: String,
        source: serde_json::Error,
    },

    #[error("failed to parse YAML config '{path}': {source}")]
    ParseYaml {
        path: String,
        source: serde_yaml::Error,
    },

    #[error("config validation failed for '{path}': {error}")]
    Validation {
        path: String,
        error: validate::ValidationError,
    },
}

/// Returns true if the path has a YAML extension (.yaml or .yml).
fn is_yaml(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"))
}

/// Load a config file, expanding `${ENV_VAR}` placeholders in string values.
///
/// Auto-detects format by file extension:
/// - `.yaml` / `.yml` → parsed as YAML
/// - everything else → parsed as JSON
///
/// # Env Var Expansion
/// Any string value containing `${VAR_NAME}` will have the placeholder replaced
/// with the environment variable's value. If the variable is not set, the
/// placeholder is left as-is and a warning is logged.
///
/// Supports multiple placeholders per string and mixed text:
/// - `"postgres://${DB_USER}:${DB_PASS}@localhost/yggdrasil"`
pub fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let path_str = path.display().to_string();

    tracing::debug!(path = %path_str, "loading config");

    let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadFile {
        path: path_str.clone(),
        source: e,
    })?;

    // Expand ${ENV_VAR} placeholders before parsing.
    let expanded = expand_env_vars(&raw);

    if is_yaml(path) {
        serde_yaml::from_str(&expanded).map_err(|e| ConfigError::ParseYaml {
            path: path_str,
            source: e,
        })
    } else {
        serde_json::from_str(&expanded).map_err(|e| ConfigError::ParseJson {
            path: path_str,
            source: e,
        })
    }
}

/// Load a config file with env expansion and validation.
///
/// Same as [`load_json`] but additionally runs `Validate::validate()` on the
/// loaded config, returning the first validation error if any.
pub fn load_json_validated<T: DeserializeOwned + Validate>(
    path: &Path,
) -> Result<T, ConfigError> {
    let config: T = load_json(path)?;

    let errors = config.validate();
    if let Some(first) = errors.into_iter().next() {
        return Err(ConfigError::Validation {
            path: path.display().to_string(),
            error: first,
        });
    }

    Ok(config)
}

/// Load a JSON config file without env var expansion (for testing or known-safe configs).
pub fn load_json_raw<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let path_str = path.display().to_string();
    let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadFile {
        path: path_str.clone(),
        source: e,
    })?;
    serde_json::from_str(&raw).map_err(|e| ConfigError::ParseJson {
        path: path_str,
        source: e,
    })
}
