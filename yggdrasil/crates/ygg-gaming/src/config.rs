use std::path::Path;

use serde::{Deserialize, Serialize};

/// Multi-host gaming/compute orchestration config.
#[derive(Debug, Clone, Deserialize)]
pub struct GamingConfig {
    pub hosts: Vec<HostConfig>,
    #[serde(default)]
    pub timeouts: TimeoutConfig,
    pub pairing_source: Option<String>,
}

impl GamingConfig {
    /// Find a VM entry by name across all hosts.
    pub fn find_vm(&self, vm_name: &str) -> Option<(&HostConfig, &VmEntry)> {
        for host in &self.hosts {
            if let Some(vm) = host.vms.iter().find(|v| v.name == vm_name) {
                return Some((host, vm));
            }
        }
        None
    }

    /// Find a container entry by name across all hosts.
    pub fn find_container(&self, ct_name: &str) -> Option<(&HostConfig, &ContainerEntry)> {
        for host in &self.hosts {
            if let Some(ct) = host.containers.iter().find(|c| c.name == ct_name) {
                return Some((host, ct));
            }
        }
        None
    }
}

/// Configuration for a single Proxmox host.
#[derive(Debug, Clone, Deserialize)]
pub struct HostConfig {
    pub name: String,
    pub proxmox: ProxmoxConfig,
    pub wol: Option<WolConfig>,
    #[serde(default)]
    pub gpus: Vec<GpuEntry>,
    #[serde(default)]
    pub vms: Vec<VmEntry>,
    #[serde(default)]
    pub containers: Vec<ContainerEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxmoxConfig {
    pub url: String,
    pub token: String,
    pub node: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WolConfig {
    pub mac: String,
    #[serde(default = "default_broadcast")]
    pub broadcast: String,
}

fn default_broadcast() -> String {
    "255.255.255.255".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GpuEntry {
    pub name: String,
    pub pci_address: String,
    pub mapping_id: String,
    pub iommu_group: u32,
    pub vendor: String,
    pub priority: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VmEntry {
    pub name: String,
    pub vmid: u32,
    pub ip: Option<String>,
    #[serde(default = "default_gpu_preference")]
    pub gpu_preference: String,
    #[serde(default = "default_hostpci_slot")]
    pub hostpci_slot: String,
    pub ssh_user: Option<String>,
    #[serde(default)]
    pub role: VmRole,
}

/// Determines launch behavior for a VM.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VmRole {
    /// Gaming VM — single GPU, Sunshine streaming, Moonlight pairing.
    Gaming {
        #[serde(default = "default_sunshine_port")]
        sunshine_port: u16,
        sunshine_creds: Option<String>,
    },
    /// Inference VM — multi-GPU, llama-server or similar API.
    Inference {
        model: String,
        #[serde(default = "default_api_port")]
        api_port: u16,
        #[serde(default = "default_gpu_count")]
        gpu_count: usize,
        #[serde(default = "default_health_endpoint")]
        health_endpoint: String,
    },
    /// Service VM — simple start/stop, no GPU assignment.
    Service,
}

impl Default for VmRole {
    fn default() -> Self {
        VmRole::Gaming {
            sunshine_port: 47990,
            sunshine_creds: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContainerEntry {
    pub name: String,
    pub vmid: u32,
    pub ip: Option<String>,
}

fn default_sunshine_port() -> u16 { 47990 }
fn default_gpu_preference() -> String { "nvidia".to_string() }
fn default_hostpci_slot() -> String { "hostpci0".to_string() }
fn default_api_port() -> u16 { 8080 }
fn default_gpu_count() -> usize { 1 }
fn default_health_endpoint() -> String { "/health".to_string() }

#[derive(Debug, Clone, Deserialize)]
pub struct TimeoutConfig {
    #[serde(default = "default_30")]
    pub wol_poll_secs: u64,
    #[serde(default = "default_5")]
    pub wol_poll_interval_secs: u64,
    #[serde(default = "default_300")]
    pub vm_start_timeout_secs: u64,
    #[serde(default = "default_10")]
    pub vm_start_poll_interval_secs: u64,
    #[serde(default = "default_120")]
    pub vm_stop_timeout_secs: u64,
    #[serde(default = "default_10")]
    pub vm_stop_poll_interval_secs: u64,
    #[serde(default = "default_60")]
    pub ssh_ready_timeout_secs: u64,
    #[serde(default = "default_5")]
    pub ssh_ready_poll_interval_secs: u64,
}

fn default_30() -> u64 { 30 }
fn default_5() -> u64 { 5 }
fn default_60() -> u64 { 60 }
fn default_300() -> u64 { 300 }
fn default_10() -> u64 { 10 }
fn default_120() -> u64 { 120 }

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            wol_poll_secs: 30,
            wol_poll_interval_secs: 5,
            vm_start_timeout_secs: 300,
            vm_start_poll_interval_secs: 10,
            vm_stop_timeout_secs: 120,
            vm_stop_poll_interval_secs: 10,
            ssh_ready_timeout_secs: 60,
            ssh_ready_poll_interval_secs: 5,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("environment variable {var} not set")]
    Env { var: String },
    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Load config from a JSON file, expanding `${VAR}` placeholders from env.
pub fn load_config(path: &Path) -> Result<GamingConfig, ConfigError> {
    let raw = std::fs::read_to_string(path)?;

    let mut expanded = raw;
    let mut pos = 0;
    loop {
        let Some(start) = expanded[pos..].find("${") else { break };
        let start = pos + start;
        let Some(end) = expanded[start..].find('}') else {
            pos = start + 2;
            continue;
        };
        let var_name = &expanded[start + 2..start + end];
        if var_name.is_empty() {
            pos = start + end + 1;
            continue;
        }
        let value = std::env::var(var_name).map_err(|_| ConfigError::Env {
            var: var_name.to_string(),
        })?;
        let new = format!("{}{}{}", &expanded[..start], value, &expanded[start + end + 1..]);
        pos = start + value.len();
        expanded = new;
    }

    let config: GamingConfig = serde_json::from_str(&expanded)?;
    Ok(config)
}
