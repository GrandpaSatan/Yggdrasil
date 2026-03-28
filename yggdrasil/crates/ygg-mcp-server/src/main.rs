//! Local MCP server binary for Yggdrasil.
//!
//! Serves only filesystem-dependent tools (sync_docs) over stdio transport.
//! Network-only tools (memory, generate, search, HA) are served by the
//! remote `ygg-mcp-remote` binary over Streamable HTTP.
//!
//! Usage:
//!   ygg-mcp-server [--config <path>]

mod config_sync;

use anyhow::{Context, Result};
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};
use ygg_domain::config::McpServerConfig;
use ygg_mcp::local_server::YggdrasilLocalServer;
use ygg_mcp::memory_merge;

/// Yggdrasil local MCP server — filesystem tools via stdio.
#[derive(Debug, Parser)]
#[command(name = "ygg-mcp-server", version, about)]
struct Args {
    /// Path to the JSON configuration file.
    #[arg(
        short,
        long,
        default_value = "configs/mcp-server/config.json",
        env = "YGG_MCP_CONFIG"
    )]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // IMPORTANT: log to stderr only. stdout is the JSON-RPC channel.
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    info!(config = %args.config.display(), "loading local MCP server configuration");

    let config: McpServerConfig =
        ygg_config::load_json(&args.config)
            .with_context(|| format!("failed to load config: {}", args.config.display()))?;

    info!(
        odin_url = %config.odin_url,
        timeout_secs = config.timeout_secs,
        "configuration loaded"
    );

    // Run config sync before creating the server (non-fatal on failure)
    if let Some(ref remote_url) = config.remote_url
        && let Err(e) = config_sync::run_startup_sync(
            remote_url,
            config.workspace_path.as_deref(),
            config.project.as_deref(),
        )
        .await
    {
        tracing::warn!(error = %e, "config sync failed — continuing with local config");
    }

    // Run memory merge before creating the server (non-fatal on failure)
    if config.remote_ssh.is_some() {
        let merge_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();

        let result = memory_merge::merge_all_project_memories(
            &merge_client,
            &config.odin_url,
            config.remote_ssh.as_deref(),
        )
        .await;

        if result.has_changes() {
            info!(summary = %result.summary(), "startup memory merge complete");
        }
        if !result.errors.is_empty() {
            for err in &result.errors {
                tracing::warn!(error = %err, "memory merge warning");
            }
        }
    }

    let server = YggdrasilLocalServer::from_config(&config);

    let (stdin, stdout) = stdio();

    info!("starting local MCP server on stdio transport");

    let running = server
        .serve((stdin, stdout))
        .await
        .context("MCP server failed during initialization handshake")?;

    info!("MCP server initialized, waiting for requests");

    running
        .waiting()
        .await
        .context("MCP server task panicked")?;

    info!("MCP server shut down cleanly");
    Ok(())
}
