use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use tracing::{info, warn};
use ygg_domain::mesh::{
    ClusterConfig, Heartbeat, MeshHello, NodeCapabilities, NodeState, NodeStatus,
};

/// Thread-safe registry of all known mesh nodes.
#[derive(Debug, Clone)]
pub struct NodeRegistry {
    /// Map of node_name → NodeState.
    nodes: Arc<DashMap<String, NodeState>>,
    /// This node's config.
    config: ClusterConfig,
}

impl NodeRegistry {
    pub fn new(config: ClusterConfig) -> Self {
        Self {
            nodes: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Register or update a node from a MeshHello handshake.
    pub fn register(&self, hello: MeshHello) {
        let name = hello.node.name.clone();
        let now = now_epoch();

        let state = NodeState {
            identity: hello.node,
            status: NodeStatus::Online,
            capabilities: hello.capabilities,
            last_heartbeat: now,
            version: hello.version,
        };

        info!(node = %name, "node registered in mesh");
        self.nodes.insert(name, state);
    }

    /// Process an incoming heartbeat. Updates last_heartbeat and sets status Online.
    pub fn heartbeat(&self, hb: Heartbeat) {
        if let Some(mut entry) = self.nodes.get_mut(&hb.node_name) {
            entry.last_heartbeat = hb.timestamp;
            entry.status = NodeStatus::Online;
        } else {
            warn!(node = %hb.node_name, "heartbeat from unknown node — ignoring");
        }
    }

    /// Check all nodes for liveness. Mark nodes as Offline if they exceeded the
    /// missed heartbeat threshold.
    pub fn check_liveness(&self) {
        let now = now_epoch();
        let timeout = self.config.heartbeat.interval_secs
            * self.config.heartbeat.missed_threshold as u64;

        for mut entry in self.nodes.iter_mut() {
            let elapsed = now.saturating_sub(entry.last_heartbeat);
            if elapsed > timeout && entry.status == NodeStatus::Online {
                warn!(
                    node = %entry.identity.name,
                    elapsed_secs = elapsed,
                    "node missed heartbeat threshold — marking offline"
                );
                entry.status = NodeStatus::Offline;
            }
        }
    }

    /// Get a snapshot of a specific node's state.
    pub fn get_node(&self, name: &str) -> Option<NodeState> {
        self.nodes.get(name).map(|e| e.value().clone())
    }

    /// Get all online nodes.
    pub fn online_nodes(&self) -> Vec<NodeState> {
        self.nodes
            .iter()
            .filter(|e| e.status == NodeStatus::Online)
            .map(|e| e.value().clone())
            .collect()
    }

    /// Get all known nodes regardless of status.
    pub fn all_nodes(&self) -> Vec<NodeState> {
        self.nodes.iter().map(|e| e.value().clone()).collect()
    }

    /// Remove a node from the registry.
    pub fn remove(&self, name: &str) -> Option<NodeState> {
        self.nodes.remove(name).map(|(_, v)| v)
    }

    /// Get this node's config.
    pub fn config(&self) -> &ClusterConfig {
        &self.config
    }

    /// Build a MeshHello for this node.
    pub fn local_hello(&self, capabilities: NodeCapabilities) -> MeshHello {
        MeshHello {
            node: self.config.node.clone(),
            capabilities,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Build a Heartbeat for this node.
    pub fn local_heartbeat(&self) -> Heartbeat {
        Heartbeat {
            node_name: self.config.node.name.clone(),
            timestamp: now_epoch(),
            load: None,
        }
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
