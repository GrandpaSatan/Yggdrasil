//! Validate implementations for Yggdrasil config structs.
//!
//! Provides concrete [`Validate`] implementations for the domain config types
//! so callers can call `config.validate()` after loading.

use ygg_domain::config::{
    HaConfig, HuginnConfig, MimirConfig, MuninnConfig, OdinConfig,
};

use crate::validate::{
    Validate, ValidationError, validate_listen_addr, validate_no_unexpanded_vars,
    validate_not_empty, validate_url,
};

impl Validate for OdinConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if let Some(e) = validate_listen_addr("listen_addr", &self.listen_addr) {
            errors.push(e);
        }
        if let Some(e) = validate_not_empty("node_name", &self.node_name) {
            errors.push(e);
        }
        if self.backends.is_empty() {
            errors.push(ValidationError::new("backends", "must have at least one backend"));
        }
        for (i, backend) in self.backends.iter().enumerate() {
            let prefix = format!("backends[{i}]");
            if let Some(e) = validate_not_empty(&format!("{prefix}.name"), &backend.name) {
                errors.push(e);
            }
            if let Some(e) = validate_url(&format!("{prefix}.url"), &backend.url) {
                errors.push(e);
            }
        }
        if let Some(e) = validate_url("mimir.url", &self.mimir.url) {
            errors.push(e);
        }
        if let Some(e) = validate_url("muninn.url", &self.muninn.url) {
            errors.push(e);
        }
        if let Some(ref ha) = self.ha {
            errors.extend(ha.validate());
        }
        // Check cloud provider API keys for unexpanded env vars
        if let Some(ref cloud) = self.cloud {
            if let Some(ref openai) = cloud.openai
                && let Some(e) = validate_no_unexpanded_vars("cloud.openai.api_key", &openai.api_key)
                {
                    errors.push(e);
                }
            if let Some(ref claude) = cloud.claude
                && let Some(e) =
                    validate_no_unexpanded_vars("cloud.claude.api_key", &claude.api_key)
                {
                    errors.push(e);
                }
            if let Some(ref gemini) = cloud.gemini
                && let Some(e) =
                    validate_no_unexpanded_vars("cloud.gemini.api_key", &gemini.api_key)
                {
                    errors.push(e);
                }
        }

        errors
    }
}

impl Validate for HaConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if let Some(e) = validate_url("ha.url", &self.url) {
            errors.push(e);
        }
        if let Some(e) = validate_not_empty("ha.token", &self.token) {
            errors.push(e);
        }
        if let Some(e) = validate_no_unexpanded_vars("ha.token", &self.token) {
            errors.push(e);
        }

        errors
    }
}

impl Validate for MimirConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if let Some(e) = validate_listen_addr("listen_addr", &self.listen_addr) {
            errors.push(e);
        }
        if let Some(e) = validate_not_empty("database_url", &self.database_url) {
            errors.push(e);
        }
        if let Some(e) = validate_no_unexpanded_vars("database_url", &self.database_url) {
            errors.push(e);
        }
        if let Some(e) = validate_url("qdrant_url", &self.qdrant_url) {
            errors.push(e);
        }
        if let Some(e) = validate_not_empty("sdr.model_dir", &self.sdr.model_dir) {
            errors.push(e);
        }

        errors
    }
}

impl Validate for HuginnConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if let Some(e) = validate_listen_addr("listen_addr", &self.listen_addr) {
            errors.push(e);
        }
        if let Some(e) = validate_not_empty("database_url", &self.database_url) {
            errors.push(e);
        }
        if let Some(e) = validate_url("qdrant_url", &self.qdrant_url) {
            errors.push(e);
        }
        if self.watch_paths.is_empty() {
            errors.push(ValidationError::new("watch_paths", "must have at least one path"));
        }

        errors
    }
}

impl Validate for MuninnConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if let Some(e) = validate_listen_addr("listen_addr", &self.listen_addr) {
            errors.push(e);
        }
        if let Some(e) = validate_not_empty("database_url", &self.database_url) {
            errors.push(e);
        }
        if let Some(e) = validate_url("qdrant_url", &self.qdrant_url) {
            errors.push(e);
        }

        errors
    }
}
