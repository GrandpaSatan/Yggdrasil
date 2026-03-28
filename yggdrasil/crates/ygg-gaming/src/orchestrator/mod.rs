mod lifecycle;
mod pairing;
pub mod types;

pub use types::*;

use tracing::{info, warn};

use crate::config::{GamingConfig, VmRole};
use crate::gpu_pool;

/// Launch a VM: WoL host if needed → assign GPU → start VM.
pub async fn launch(
    config: &GamingConfig,
    vm_name: &str,
) -> Result<LaunchResult, OrchestratorError> {
    let (host, vm) = config
        .find_vm(vm_name)
        .ok_or_else(|| OrchestratorError::VmNotFound(vm_name.to_string()))?;

    let client = lifecycle::make_client(host);
    let node = &host.proxmox.node;

    // Step 1: Wake host if needed
    let online = lifecycle::wake_host(
        &client,
        host,
        config.timeouts.wol_poll_secs,
        config.timeouts.wol_poll_interval_secs,
    )
    .await?;

    if !online {
        return Ok(LaunchResult::ServerOffline {
            host: host.name.clone(),
        });
    }

    // Step 2: Check if VM is already running
    let vm_status = client.vm_status(node, vm.vmid).await?;
    if vm_status.status == "running" {
        let gpu =
            gpu_pool::gpu_assigned_to_vm(&host.gpus, &client, node, vm.vmid).await?;
        if gpu.is_some() {
            info!(vm = vm_name, host = %host.name, "VM already running with GPU assigned");
            return Ok(LaunchResult::AlreadyRunning {
                vm_name: vm_name.to_string(),
                ip: vm.ip.clone(),
            });
        }
        // For Service VMs, running without GPU is normal
        if matches!(vm.role, VmRole::Service) {
            return Ok(LaunchResult::AlreadyRunning {
                vm_name: vm_name.to_string(),
                ip: vm.ip.clone(),
            });
        }
        warn!(vm = vm_name, "VM running but no GPU assigned");
    }

    // Step 3+4: GPU assignment depends on role
    let gpu_name = match &vm.role {
        VmRole::Gaming { .. } => {
            // Single GPU assignment
            let gpu =
                gpu_pool::find_available_gpu(&host.gpus, &client, node, &vm.gpu_preference)
                    .await?;

            let Some(gpu) = gpu else {
                let vms = client
                    .list_vms(node)
                    .await?
                    .iter()
                    .filter(|v| v.status == "running")
                    .map(|v| v.name.clone().unwrap_or_else(|| format!("VM {}", v.vmid)))
                    .collect();
                return Ok(LaunchResult::NoGpuAvailable { running_vms: vms });
            };

            let name = gpu.name.clone();
            if vm_status.status != "running" {
                let pci_value = format!("mapping={},pcie=1,x-vga=1", gpu.mapping_id);
                info!(vm = vm_name, gpu = %name, slot = %vm.hostpci_slot, "assigning GPU");
                client
                    .set_vm_config(node, vm.vmid, &[(&vm.hostpci_slot, &pci_value)])
                    .await?;
            }
            name
        }
        VmRole::Inference { gpu_count, .. } => {
            // Multi-GPU assignment
            let mut assigned_names = Vec::new();
            for i in 0..*gpu_count {
                let gpu = gpu_pool::find_available_gpu(
                    &host.gpus, &client, node, &vm.gpu_preference,
                )
                .await?;

                let Some(gpu) = gpu else {
                    let vms = client
                        .list_vms(node)
                        .await?
                        .iter()
                        .filter(|v| v.status == "running")
                        .map(|v| v.name.clone().unwrap_or_else(|| format!("VM {}", v.vmid)))
                        .collect();
                    return Ok(LaunchResult::NoGpuAvailable { running_vms: vms });
                };

                assigned_names.push(gpu.name.clone());
                if vm_status.status != "running" {
                    let slot = format!("hostpci{i}");
                    let pci_value = format!("mapping={},pcie=1", gpu.mapping_id);
                    info!(vm = vm_name, gpu = %gpu.name, slot = %slot, "assigning GPU {}/{}", i + 1, gpu_count);
                    client
                        .set_vm_config(node, vm.vmid, &[(&slot, &pci_value)])
                        .await?;
                }
            }
            assigned_names.join(" + ")
        }
        VmRole::Service => "none".to_string(),
    };

    // Step 5: Start VM
    if vm_status.status != "running" {
        info!(vm = vm_name, "starting VM");
        client.start_vm(node, vm.vmid).await?;
    }

    // Step 6: Poll until running
    lifecycle::wait_for_vm_status(
        &client,
        node,
        vm.vmid,
        "running",
        config.timeouts.vm_start_timeout_secs,
        config.timeouts.vm_start_poll_interval_secs,
    )
    .await?;

    info!(vm = vm_name, host = %host.name, "VM is now running");

    // Step 7: Post-boot actions depend on role
    match &vm.role {
        VmRole::Gaming { .. } => {
            // Deploy Sunshine pairing data if configured
            if let (Some(pairing_src), Some(ip), Some(ssh_user)) =
                (&config.pairing_source, &vm.ip, &vm.ssh_user)
            {
                info!(vm = vm_name, "waiting for SSH before deploying pairing data");
                match pairing::wait_for_ssh(
                    ip,
                    config.timeouts.ssh_ready_timeout_secs,
                    config.timeouts.ssh_ready_poll_interval_secs,
                )
                .await
                {
                    Ok(()) => {
                        if let Err(e) = pairing::deploy_pairing(pairing_src, ssh_user, ip).await {
                            warn!(vm = vm_name, error = %e, "failed to deploy pairing (non-fatal)");
                        }
                    }
                    Err(e) => {
                        warn!(vm = vm_name, error = %e, "SSH readiness failed (non-fatal)");
                    }
                }
            }
        }
        VmRole::Inference { api_port, health_endpoint, .. } => {
            // Wait for inference API health endpoint
            if let Some(ip) = &vm.ip {
                let url = format!("http://{ip}:{api_port}{health_endpoint}");
                info!(vm = vm_name, url = %url, "waiting for inference API");
                let deadline = std::time::Instant::now()
                    + std::time::Duration::from_secs(config.timeouts.ssh_ready_timeout_secs);
                let interval = std::time::Duration::from_secs(config.timeouts.ssh_ready_poll_interval_secs);
                let http = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .unwrap_or_default();

                loop {
                    if let Ok(resp) = http.get(&url).send().await {
                        if resp.status().is_success() {
                            info!(vm = vm_name, "inference API is ready");
                            break;
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        warn!(vm = vm_name, "inference API not ready within timeout (non-fatal)");
                        break;
                    }
                    tokio::time::sleep(interval).await;
                }
            }
        }
        VmRole::Service => {}
    }

    Ok(LaunchResult::Started {
        vm_name: vm_name.to_string(),
        host: host.name.clone(),
        gpu_name,
        ip: vm.ip.clone(),
    })
}

/// Stop a VM and release its GPU.
pub async fn stop(
    config: &GamingConfig,
    vm_name: &str,
) -> Result<(), OrchestratorError> {
    let (host, vm) = config
        .find_vm(vm_name)
        .ok_or_else(|| OrchestratorError::VmNotFound(vm_name.to_string()))?;

    let client = lifecycle::make_client(host);
    let node = &host.proxmox.node;

    let vm_status = client.vm_status(node, vm.vmid).await?;
    if vm_status.status == "stopped" {
        info!(vm = vm_name, "VM already stopped");
        // Clean up any leftover GPU assignments
        if let VmRole::Inference { gpu_count, .. } = &vm.role {
            let slots: Vec<String> = (0..*gpu_count).map(|i| format!("hostpci{i}")).collect();
            let slot_refs: Vec<&str> = slots.iter().map(|s| s.as_str()).collect();
            let _ = client.delete_vm_config_keys(node, vm.vmid, &slot_refs).await;
        } else {
            let _ = client.delete_vm_config_keys(node, vm.vmid, &[&vm.hostpci_slot]).await;
        }
        return Ok(());
    }

    info!(vm = vm_name, host = %host.name, "shutting down VM");
    client.stop_vm(node, vm.vmid).await?;

    lifecycle::wait_for_vm_status(
        &client,
        node,
        vm.vmid,
        "stopped",
        config.timeouts.vm_stop_timeout_secs,
        config.timeouts.vm_stop_poll_interval_secs,
    )
    .await?;

    // Release GPU(s) based on role
    match &vm.role {
        VmRole::Gaming { .. } => {
            info!(vm = vm_name, slot = %vm.hostpci_slot, "releasing GPU");
            client
                .delete_vm_config_keys(node, vm.vmid, &[&vm.hostpci_slot])
                .await?;
        }
        VmRole::Inference { gpu_count, .. } => {
            let slots: Vec<String> = (0..*gpu_count).map(|i| format!("hostpci{i}")).collect();
            let slot_refs: Vec<&str> = slots.iter().map(|s| s.as_str()).collect();
            info!(vm = vm_name, slots = ?slot_refs, "releasing GPUs");
            client
                .delete_vm_config_keys(node, vm.vmid, &slot_refs)
                .await?;
        }
        VmRole::Service => {}
    }

    Ok(())
}

/// Stop a container.
pub async fn stop_container(
    config: &GamingConfig,
    ct_name: &str,
) -> Result<(), OrchestratorError> {
    let (host, ct) = config
        .find_container(ct_name)
        .ok_or_else(|| OrchestratorError::ContainerNotFound(ct_name.to_string()))?;

    let client = lifecycle::make_client(host);
    let node = &host.proxmox.node;

    info!(ct = ct_name, host = %host.name, "stopping container");
    client.stop_container(node, ct.vmid).await?;
    Ok(())
}

/// Start a container (wake host first if needed).
pub async fn start_container(
    config: &GamingConfig,
    ct_name: &str,
) -> Result<(), OrchestratorError> {
    let (host, ct) = config
        .find_container(ct_name)
        .ok_or_else(|| OrchestratorError::ContainerNotFound(ct_name.to_string()))?;

    let client = lifecycle::make_client(host);
    let node = &host.proxmox.node;

    let online = lifecycle::wake_host(
        &client,
        host,
        config.timeouts.wol_poll_secs,
        config.timeouts.wol_poll_interval_secs,
    )
    .await?;

    if !online {
        return Err(OrchestratorError::Timeout {
            action: format!("wake host '{}'", host.name),
            secs: config.timeouts.wol_poll_secs,
        });
    }

    info!(ct = ct_name, host = %host.name, "starting container");
    client.start_container(node, ct.vmid).await?;
    Ok(())
}

/// Pair a Moonlight client with a VM's Sunshine.
pub async fn pair(
    config: &GamingConfig,
    vm_name: &str,
    pin: &str,
) -> Result<String, OrchestratorError> {
    let (_host, vm) = config
        .find_vm(vm_name)
        .ok_or_else(|| OrchestratorError::VmNotFound(vm_name.to_string()))?;

    pairing::pair_pin(vm, pin).await
}

/// Get status of all hosts, VMs, containers, and GPU assignments.
pub async fn status_all(config: &GamingConfig) -> Result<SystemStatus, OrchestratorError> {
    let mut hosts = Vec::new();

    for host in &config.hosts {
        let client = lifecycle::make_client(host);
        let node = &host.proxmox.node;

        let online = client.node_online(node).await.unwrap_or(false);
        let mut vms = Vec::new();
        let mut containers = Vec::new();

        if online {
            // VMs
            for vm_entry in &host.vms {
                let status = match client.vm_status(node, vm_entry.vmid).await {
                    Ok(s) => s.status,
                    Err(_) => "unknown".to_string(),
                };

                let gpu =
                    gpu_pool::gpu_assigned_to_vm(&host.gpus, &client, node, vm_entry.vmid)
                        .await
                        .ok()
                        .flatten()
                        .map(|g| g.name);

                vms.push(VmStatusEntry {
                    name: vm_entry.name.clone(),
                    vmid: vm_entry.vmid,
                    status,
                    gpu,
                    ip: vm_entry.ip.clone(),
                });
            }

            // Containers
            for ct_entry in &host.containers {
                let status = match client.container_status(node, ct_entry.vmid).await {
                    Ok(s) => s.status,
                    Err(_) => "unknown".to_string(),
                };

                containers.push(ContainerStatusEntry {
                    name: ct_entry.name.clone(),
                    vmid: ct_entry.vmid,
                    status,
                    ip: ct_entry.ip.clone(),
                });
            }
        }

        hosts.push(HostStatus {
            name: host.name.clone(),
            online,
            vms,
            containers,
        });
    }

    Ok(SystemStatus { hosts })
}

/// Get GPU pool status across all hosts.
pub async fn list_gpus(config: &GamingConfig) -> Result<Vec<GpuPoolStatus>, OrchestratorError> {
    let mut all = Vec::new();

    for host in &config.hosts {
        if host.gpus.is_empty() {
            continue;
        }
        let client = lifecycle::make_client(host);
        let node = &host.proxmox.node;

        let statuses = gpu_pool::gpu_status_all(&host.gpus, &client, node).await?;

        all.extend(
            statuses
                .into_iter()
                .map(|gs| GpuPoolStatus::from_gpu_status(&host.name, gs)),
        );
    }

    Ok(all)
}
