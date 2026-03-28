mod config;
mod gpu_pool;
mod orchestrator;

use std::path::PathBuf;
use std::process;

use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(name = "ygg-gaming", version, about = "Multi-host compute orchestrator")]
struct Cli {
    #[arg(
        short,
        long,
        default_value = "configs/gaming/config.json",
        env = "YGG_GAMING_CONFIG"
    )]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Launch a VM (wake host if needed, assign GPU, start VM).
    Launch { vm_name: String },
    /// Stop a VM and release its GPU.
    Stop { vm_name: String },
    /// Start a container (wake host if needed).
    StartCt { ct_name: String },
    /// Stop a container.
    StopCt { ct_name: String },
    /// Show status of all hosts, VMs, containers, and GPUs.
    Status,
    /// List GPU pool with availability across all hosts.
    ListGpus,
    /// Pair a Moonlight client with a VM's Sunshine (enters PIN via SSH).
    Pair { vm_name: String, pin: String },
}

#[tokio::main]
async fn main() {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let cfg = match config::load_config(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config {}: {e}", cli.config.display());
            process::exit(1);
        }
    };

    match cli.command {
        Command::Launch { vm_name } => match orchestrator::launch(&cfg, &vm_name).await {
            Ok(orchestrator::LaunchResult::Started {
                vm_name,
                host,
                gpu_name,
                ip,
            }) => {
                println!("✓ {vm_name} started on {host} with {gpu_name}");
                if let Some(ip) = ip {
                    println!("  Connect via Moonlight: {ip}");
                }
            }
            Ok(orchestrator::LaunchResult::AlreadyRunning { vm_name, ip }) => {
                println!("✓ {vm_name} is already running");
                if let Some(ip) = ip {
                    println!("  Connect via Moonlight: {ip}");
                }
            }
            Ok(orchestrator::LaunchResult::ServerOffline { host }) => {
                eprintln!("✗ {host} did not wake up within timeout");
                process::exit(1);
            }
            Ok(orchestrator::LaunchResult::NoGpuAvailable { running_vms }) => {
                eprintln!(
                    "✗ No GPU available — all in use by: {}",
                    running_vms.join(", ")
                );
                process::exit(1);
            }
            Err(e) => {
                eprintln!("✗ Launch failed: {e}");
                process::exit(1);
            }
        },

        Command::Stop { vm_name } => match orchestrator::stop(&cfg, &vm_name).await {
            Ok(()) => println!("✓ {vm_name} stopped and GPU released"),
            Err(e) => {
                eprintln!("✗ Stop failed: {e}");
                process::exit(1);
            }
        },

        Command::StartCt { ct_name } => {
            match orchestrator::start_container(&cfg, &ct_name).await {
                Ok(()) => println!("✓ {ct_name} started"),
                Err(e) => {
                    eprintln!("✗ Start container failed: {e}");
                    process::exit(1);
                }
            }
        }

        Command::StopCt { ct_name } => {
            match orchestrator::stop_container(&cfg, &ct_name).await {
                Ok(()) => println!("✓ {ct_name} stopped"),
                Err(e) => {
                    eprintln!("✗ Stop container failed: {e}");
                    process::exit(1);
                }
            }
        }

        Command::Status => match orchestrator::status_all(&cfg).await {
            Ok(status) => {
                for host in &status.hosts {
                    println!(
                        "── {} ({}) ──",
                        host.name,
                        if host.online { "ONLINE" } else { "OFFLINE" }
                    );
                    if !host.vms.is_empty() {
                        println!(
                            "  {:<12} {:>6} {:<10} {:<20} IP",
                            "VM", "VMID", "Status", "GPU"
                        );
                        for vm in &host.vms {
                            println!(
                                "  {:<12} {:>6} {:<10} {:<20} {}",
                                vm.name,
                                vm.vmid,
                                vm.status,
                                vm.gpu.as_deref().unwrap_or("-"),
                                vm.ip.as_deref().unwrap_or("-"),
                            );
                        }
                    }
                    if !host.containers.is_empty() {
                        println!(
                            "  {:<12} {:>6} {:<10} IP",
                            "Container", "VMID", "Status"
                        );
                        for ct in &host.containers {
                            println!(
                                "  {:<12} {:>6} {:<10} {}",
                                ct.name,
                                ct.vmid,
                                ct.status,
                                ct.ip.as_deref().unwrap_or("-"),
                            );
                        }
                    }
                    println!();
                }
            }
            Err(e) => {
                eprintln!("✗ Status failed: {e}");
                process::exit(1);
            }
        },

        Command::ListGpus => match orchestrator::list_gpus(&cfg).await {
            Ok(gpus) => {
                println!(
                    "{:<10} {:<20} {:<16} {:<8} Status",
                    "Host", "GPU", "PCI Address", "Vendor"
                );
                println!("{}", "-".repeat(70));
                for g in &gpus {
                    let status = g
                        .assigned_to
                        .as_deref()
                        .map(|n| format!("→ {n}"))
                        .unwrap_or_else(|| "FREE".to_string());
                    println!(
                        "{:<10} {:<20} {:<16} {:<8} {}",
                        g.host, g.name, g.pci_address, g.vendor, status
                    );
                }
            }
            Err(e) => {
                eprintln!("✗ GPU list failed: {e}");
                process::exit(1);
            }
        },

        Command::Pair { vm_name, pin } => match orchestrator::pair(&cfg, &vm_name, &pin).await {
            Ok(msg) => println!("✓ {msg}"),
            Err(e) => {
                eprintln!("✗ Pair failed: {e}");
                process::exit(1);
            }
        },
    }
}
