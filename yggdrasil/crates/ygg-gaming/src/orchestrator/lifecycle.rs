use std::time::{Duration, Instant};

use tracing::{info, warn};
use ygg_energy::proxmox::ProxmoxClient;
use ygg_energy::wol;

use crate::config::HostConfig;

use super::types::OrchestratorError;

/// Create a ProxmoxClient from a host config.
pub fn make_client(host: &HostConfig) -> ProxmoxClient {
    ProxmoxClient::new(host.proxmox.url.clone(), host.proxmox.token.clone())
}

/// Wake the host server via WoL if it's offline, polling until it comes online.
/// Returns Ok(true) if host is online (or was woken), Ok(false) if it didn't wake.
/// If the host has no WoL config, just checks if it's already online.
pub async fn wake_host(
    client: &ProxmoxClient,
    host: &HostConfig,
    poll_secs: u64,
    poll_interval_secs: u64,
) -> Result<bool, OrchestratorError> {
    let node = &host.proxmox.node;
    let online = client.node_online(node).await?;
    if online {
        return Ok(true);
    }

    let Some(wol_config) = &host.wol else {
        warn!(host = %host.name, "host offline and no WoL configured");
        return Ok(false);
    };

    info!(host = %host.name, mac = %wol_config.mac, "host offline — sending Wake-on-LAN");
    wol::send_wol(&wol_config.mac, &wol_config.broadcast)?;

    let deadline = Instant::now() + Duration::from_secs(poll_secs);
    let interval = Duration::from_secs(poll_interval_secs);

    loop {
        tokio::time::sleep(interval).await;
        if client.node_online(node).await.unwrap_or(false) {
            info!(host = %host.name, "host is now online");
            return Ok(true);
        }
        if Instant::now() >= deadline {
            warn!(host = %host.name, "host did not come online within timeout");
            return Ok(false);
        }
    }
}

/// Poll until a VM reaches the target status, or timeout.
pub async fn wait_for_vm_status(
    client: &ProxmoxClient,
    node: &str,
    vmid: u32,
    target_status: &str,
    timeout_secs: u64,
    poll_interval_secs: u64,
) -> Result<(), OrchestratorError> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let interval = Duration::from_secs(poll_interval_secs);

    loop {
        tokio::time::sleep(interval).await;
        let status = client.vm_status(node, vmid).await?;
        if status.status == target_status {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(OrchestratorError::Timeout {
                action: format!("reach '{target_status}'"),
                secs: timeout_secs,
            });
        }
    }
}
