use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::config::{VmEntry, VmRole};

use super::types::OrchestratorError;

/// Wait for SSH to become available on a VM by probing TCP port 22.
pub async fn wait_for_ssh(
    ip: &str,
    timeout_secs: u64,
    poll_interval_secs: u64,
) -> Result<(), OrchestratorError> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let interval = Duration::from_secs(poll_interval_secs);
    let addr = format!("{ip}:22");

    loop {
        match tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        {
            Ok(Ok(_)) => {
                info!(ip = ip, "SSH is ready");
                return Ok(());
            }
            _ => {
                if Instant::now() >= deadline {
                    warn!(ip = ip, "SSH readiness timed out after {timeout_secs}s");
                    return Err(OrchestratorError::Pairing(format!(
                        "SSH not ready on {ip} after {timeout_secs}s"
                    )));
                }
                tokio::time::sleep(interval).await;
            }
        }
    }
}

/// Deploy Sunshine pairing data to a VM via SSH/rsync.
/// Copies sunshine_state.json and credentials/ to ~/.config/sunshine/ on the VM,
/// then restarts the Sunshine service.
pub async fn deploy_pairing(
    source_dir: &str,
    ssh_user: &str,
    vm_ip: &str,
) -> Result<(), OrchestratorError> {
    let dest = format!("{ssh_user}@{vm_ip}");

    // rsync the pairing directory contents
    let status = tokio::process::Command::new("rsync")
        .args([
            "-az",
            "--timeout=10",
            &format!("{source_dir}/sunshine_state.json"),
            &format!("{dest}:.config/sunshine/sunshine_state.json"),
        ])
        .status()
        .await
        .map_err(|e| OrchestratorError::Pairing(format!("rsync state failed: {e}")))?;

    if !status.success() {
        return Err(OrchestratorError::Pairing(
            "rsync state failed".to_string(),
        ));
    }

    let status = tokio::process::Command::new("rsync")
        .args([
            "-az",
            "--timeout=10",
            &format!("{source_dir}/credentials/"),
            &format!("{dest}:.config/sunshine/credentials/"),
        ])
        .status()
        .await
        .map_err(|e| OrchestratorError::Pairing(format!("rsync creds failed: {e}")))?;

    if !status.success() {
        return Err(OrchestratorError::Pairing(
            "rsync creds failed".to_string(),
        ));
    }

    // Restart Sunshine to pick up new pairing data
    let _ = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=5",
            &dest,
            "systemctl --user restart sunshine.service",
        ])
        .status()
        .await;

    info!("Sunshine pairing data deployed to {vm_ip}");
    Ok(())
}

/// Pair a Moonlight client with a VM's Sunshine by entering the 4-digit PIN.
/// SSHs into the VM and POSTs the PIN to the Sunshine API.
pub async fn pair_pin(
    vm: &VmEntry,
    pin: &str,
) -> Result<String, OrchestratorError> {
    let ip = vm
        .ip
        .as_deref()
        .ok_or_else(|| {
            OrchestratorError::Pairing(format!("VM '{}' has no IP configured", vm.name))
        })?;

    let ssh_user = vm.ssh_user.as_deref().ok_or_else(|| {
        OrchestratorError::Pairing(format!("VM '{}' has no ssh_user configured", vm.name))
    })?;

    let VmRole::Gaming { sunshine_port, sunshine_creds } = &vm.role else {
        return Err(OrchestratorError::Pairing(format!(
            "VM '{}' is not a gaming VM — pairing only works with Sunshine",
            vm.name
        )));
    };
    let creds = sunshine_creds.as_deref().ok_or_else(|| {
        OrchestratorError::Pairing(format!(
            "VM '{}' has no sunshine_creds configured",
            vm.name
        ))
    })?;
    let port = *sunshine_port;

    info!(vm = %vm.name, ip = ip, "pairing Moonlight via Sunshine PIN");

    let output = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=5",
            &format!("{ssh_user}@{ip}"),
            &format!(
                "curl -sk -u {creds} -X POST \"https://localhost:{port}/api/pin\" \
                 -H \"Content-Type: application/json\" -d '{{\"pin\": \"{pin}\"}}'"
            ),
        ])
        .output()
        .await
        .map_err(|e| OrchestratorError::Pairing(format!("SSH failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("\"status\":true") {
        info!(vm = %vm.name, "Moonlight pairing successful");
        Ok(format!("Pairing successful — connect via Moonlight to {ip}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(OrchestratorError::Pairing(format!(
            "Sunshine returned: {stdout} {stderr}"
        )))
    }
}
