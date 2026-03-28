use serde::Serialize;

use crate::gpu_pool;

#[derive(Debug, Serialize)]
pub enum LaunchResult {
    Started {
        vm_name: String,
        host: String,
        gpu_name: String,
        ip: Option<String>,
    },
    AlreadyRunning {
        vm_name: String,
        ip: Option<String>,
    },
    ServerOffline {
        host: String,
    },
    NoGpuAvailable {
        running_vms: Vec<String>,
    },
}

/// Status across all managed hosts.
#[derive(Debug, Serialize)]
pub struct SystemStatus {
    pub hosts: Vec<HostStatus>,
}

#[derive(Debug, Serialize)]
pub struct HostStatus {
    pub name: String,
    pub online: bool,
    pub vms: Vec<VmStatusEntry>,
    pub containers: Vec<ContainerStatusEntry>,
}

#[derive(Debug, Serialize)]
pub struct VmStatusEntry {
    pub name: String,
    pub vmid: u32,
    pub status: String,
    pub gpu: Option<String>,
    pub ip: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ContainerStatusEntry {
    pub name: String,
    pub vmid: u32,
    pub status: String,
    pub ip: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GpuPoolStatus {
    pub host: String,
    pub name: String,
    pub pci_address: String,
    pub vendor: String,
    pub priority: u32,
    pub assigned_to: Option<String>,
}

impl GpuPoolStatus {
    pub fn from_gpu_status(host: &str, gs: gpu_pool::GpuStatus) -> Self {
        Self {
            host: host.to_string(),
            name: gs.gpu.name,
            pci_address: gs.gpu.pci_address,
            vendor: gs.gpu.vendor,
            priority: gs.gpu.priority,
            assigned_to: gs.assigned_to.map(|(_, name)| name),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("VM '{0}' not found in config")]
    VmNotFound(String),
    #[error("container '{0}' not found in config")]
    ContainerNotFound(String),
    #[error("Proxmox error: {0}")]
    Proxmox(#[from] ygg_energy::proxmox::ProxmoxError),
    #[error("GPU pool error: {0}")]
    GpuPool(#[from] gpu_pool::GpuPoolError),
    #[error("WoL error: {0}")]
    Wol(#[from] ygg_energy::wol::WolError),
    #[error("timeout waiting for VM to {action} (waited {secs}s)")]
    Timeout { action: String, secs: u64 },
    #[error("pairing failed: {0}")]
    Pairing(String),
}
