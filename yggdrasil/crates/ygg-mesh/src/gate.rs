use tracing::debug;
use ygg_domain::mesh::{GateConfig, GatePolicy};

/// Evaluates gate policy rules to determine if a request should be allowed.
pub struct Gate {
    config: GateConfig,
}

impl Gate {
    pub fn new(config: GateConfig) -> Self {
        Self { config }
    }

    /// Check if a request from `source_node` to invoke `tool_name` is permitted.
    /// Returns true if the request is allowed.
    pub fn check(&self, source_node: &str, tool_name: &str) -> bool {
        for rule in &self.config.rules {
            if glob_matches(&rule.source, source_node) && glob_matches(&rule.tool, tool_name) {
                let allowed = rule.policy == GatePolicy::Allow;
                debug!(
                    source = source_node,
                    tool = tool_name,
                    policy = ?rule.policy,
                    "gate rule matched"
                );
                return allowed;
            }
        }

        // No rule matched — apply default policy.
        let allowed = self.config.default_policy == GatePolicy::Allow;
        debug!(
            source = source_node,
            tool = tool_name,
            default = ?self.config.default_policy,
            "gate: no rule matched, using default policy"
        );
        allowed
    }
}

/// Simple glob matching supporting "*" as a wildcard.
fn glob_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    glob_match::glob_match(pattern, value)
}
