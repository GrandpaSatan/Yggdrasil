//! Configuration validation utilities.
//!
//! Provides a [`Validate`] trait and common validators for network addresses,
//! URLs, port conflicts, and required fields.

use std::collections::HashSet;
use std::net::SocketAddr;

/// Validation error with context about what failed.
#[derive(Debug, thiserror::Error)]
#[error("{field}: {message}")]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// Trait for validatable configuration structs.
pub trait Validate {
    /// Validate the configuration, returning all errors found.
    fn validate(&self) -> Vec<ValidationError>;

    /// Validate and return `Ok(())` or the first error.
    fn validate_or_err(&self) -> Result<(), ValidationError> {
        let errors = self.validate();
        if let Some(e) = errors.into_iter().next() {
            Err(e)
        } else {
            Ok(())
        }
    }
}

/// Validate that a string parses as a valid socket address (ip:port).
pub fn validate_listen_addr(field: &str, addr: &str) -> Option<ValidationError> {
    if addr.parse::<SocketAddr>().is_err() {
        Some(ValidationError::new(
            field,
            format!("invalid listen address: '{addr}' (expected ip:port)"),
        ))
    } else {
        None
    }
}

/// Validate that a string looks like a valid URL (starts with http:// or https://).
pub fn validate_url(field: &str, url: &str) -> Option<ValidationError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        Some(ValidationError::new(
            field,
            format!("invalid URL: '{url}' (must start with http:// or https://)"),
        ))
    } else {
        None
    }
}

/// Validate that a string is not empty.
pub fn validate_not_empty(field: &str, value: &str) -> Option<ValidationError> {
    if value.trim().is_empty() {
        Some(ValidationError::new(field, "must not be empty"))
    } else {
        None
    }
}

/// Validate that a path string doesn't contain unexpanded env var placeholders.
pub fn validate_no_unexpanded_vars(field: &str, value: &str) -> Option<ValidationError> {
    if value.contains("${") {
        Some(ValidationError::new(
            field,
            format!("contains unexpanded env var placeholder in '{value}'"),
        ))
    } else {
        None
    }
}

/// Check a collection of listen addresses for port conflicts.
/// Returns errors for any duplicate port bindings.
pub fn validate_no_port_conflicts(
    addrs: &[(&str, &str)], // (field_name, addr_string)
) -> Vec<ValidationError> {
    let mut seen = HashSet::new();
    let mut errors = Vec::new();

    for (field, addr) in addrs {
        if let Ok(sa) = addr.parse::<SocketAddr>() {
            let port = sa.port();
            if !seen.insert(port) {
                errors.push(ValidationError::new(
                    *field,
                    format!("port {port} conflicts with another service"),
                ));
            }
        }
    }

    errors
}
