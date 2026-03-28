//! LLM-based memory merge for multi-workstation Claude Code setups.
//!
//! When working on multiple workstations, Claude Code auto-memory files
//! (MEMORY.md + topic .md files) sync via rsync `--update` — newest timestamp
//! wins per file. If both workstations modify the same file between sync cycles,
//! the older workstation's changes are silently lost.
//!
//! This module detects diverged memory files (local vs remote) and uses Odin's
//! LLM endpoint to intelligently merge both versions, preserving all unique info.
//!
//! ## Integration Points
//!
//! - **Startup:** Called from `config_sync::run_startup_sync()` automatically.
//! - **Tool:** Exposed as `memory_merge_tool` on the local MCP server for on-demand use.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a memory merge operation.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MergeResult {
    /// Number of files merged via LLM.
    pub llm_merged: usize,
    /// Number of files merged via text fallback (LLM unavailable or failed).
    pub text_merged: usize,
    /// Number of remote-only files copied to local.
    pub copied: usize,
    /// Number of files that were identical (no action needed).
    pub identical: usize,
    /// Errors encountered (non-fatal).
    pub errors: Vec<String>,
}

impl MergeResult {
    pub fn summary(&self) -> String {
        let total = self.llm_merged + self.text_merged + self.copied;
        if total == 0 {
            "Memory sync: all files identical, no merge needed.".to_string()
        } else {
            format!(
                "Memory merge: {} LLM-merged, {} text-merged, {} copied from remote.",
                self.llm_merged, self.text_merged, self.copied
            )
        }
    }

    pub fn has_changes(&self) -> bool {
        self.llm_merged + self.text_merged + self.copied > 0
    }
}

/// Classification of a file that exists on both sides.
#[derive(Debug)]
enum FileState {
    Identical,
    LocalOnly,
    RemoteOnly,
    Diverged,
}

/// Odin chat completion response (subset).
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run memory merge for all project memory directories.
///
/// Compares local `.sync-cache` memory files against a remote source,
/// then uses Odin's LLM to merge diverged files.
///
/// # Arguments
/// - `client`: HTTP client for Odin calls
/// - `odin_url`: Base URL for Odin (e.g. "http://<munin-ip>:8080")
/// - `remote_base`: SSH-accessible remote base for rsync (e.g. "user@munin:/opt/yggdrasil/claude-config")
///
/// Returns `MergeResult` with counts of actions taken.
pub async fn merge_all_project_memories(
    client: &Client,
    odin_url: &str,
    remote_base: Option<&str>,
) -> MergeResult {
    let mut result = MergeResult::default();

    let sync_cache = home_dir().join(".claude").join(".sync-cache");
    if !sync_cache.exists() {
        info!("no .sync-cache directory — skipping memory merge");
        return result;
    }

    // If no remote base, we can't pull remote files
    let remote_base = match remote_base {
        Some(rb) => rb.to_string(),
        None => {
            info!("no remote_base configured — skipping memory merge");
            return result;
        }
    };

    // Create staging directory
    let staging = PathBuf::from("/tmp/ygg-memory-merge-staging");
    let _ = fs::remove_dir_all(&staging);
    if let Err(e) = fs::create_dir_all(&staging) {
        result.errors.push(format!("failed to create staging dir: {e}"));
        return result;
    }

    // Discover all project memory directories (local + remote)
    let local_projects = discover_local_projects(&sync_cache);
    let remote_projects = discover_remote_projects(&remote_base).await;

    let all_projects: HashSet<String> = local_projects
        .iter()
        .chain(remote_projects.iter())
        .cloned()
        .collect();

    if all_projects.is_empty() {
        info!("no project memories found — skipping merge");
        let _ = fs::remove_dir_all(&staging);
        return result;
    }

    // Pull remote memories to staging
    for encoded in &all_projects {
        if let Err(e) = pull_remote_memory(&remote_base, encoded, &staging).await {
            warn!(project = %encoded, error = %e, "failed to pull remote memory");
            result.errors.push(format!("{encoded}: pull failed: {e}"));
        }
    }

    // Compare and merge each project's memory files
    for encoded in &all_projects {
        let local_mem = sync_cache.join("projects").join(encoded).join("memory");
        let stage_mem = staging.join(encoded).join("memory");

        // Collect all .md files from both sides
        let all_files = collect_md_files(&local_mem, &stage_mem);

        for filename in &all_files {
            let local_file = local_mem.join(filename);
            let remote_file = stage_mem.join(filename);

            match classify_file(&local_file, &remote_file) {
                FileState::Identical => {
                    result.identical += 1;
                }
                FileState::LocalOnly => {
                    // Will propagate on next sync push — no action
                }
                FileState::RemoteOnly => {
                    // Copy from staging to local
                    if let Err(e) = copy_remote_to_local(&remote_file, &local_file) {
                        result.errors.push(format!("{encoded}/{filename}: copy failed: {e}"));
                    } else {
                        info!(project = %encoded, file = %filename, "copied remote-only file");
                        result.copied += 1;
                    }
                }
                FileState::Diverged => {
                    // Back up local before merge
                    let backup = local_file.with_extension("md.pre-merge");
                    let _ = fs::copy(&local_file, &backup);

                    let local_content = fs::read_to_string(&local_file).unwrap_or_default();
                    let remote_content = fs::read_to_string(&remote_file).unwrap_or_default();

                    // Skip files larger than 8KB for LLM merge
                    let use_llm = local_content.len() < 8192 && remote_content.len() < 8192;

                    let merged = if use_llm {
                        llm_merge(client, odin_url, filename, &local_content, &remote_content)
                            .await
                    } else {
                        None
                    };

                    if let Some(content) = merged {
                        // Validate: not empty, reasonable length
                        if content.len() >= 20 && validate_merged(&local_file, &content) {
                            if let Err(e) = fs::write(&local_file, &content) {
                                result.errors.push(format!("{encoded}/{filename}: write failed: {e}"));
                                // Restore backup
                                let _ = fs::copy(&backup, &local_file);
                            } else {
                                info!(project = %encoded, file = %filename, "LLM-merged");
                                result.llm_merged += 1;
                            }
                        } else {
                            // LLM output invalid — fall back
                            let fallback = text_fallback_merge(filename, &local_content, &remote_content);
                            if let Err(e) = fs::write(&local_file, &fallback) {
                                result.errors.push(format!("{encoded}/{filename}: fallback write failed: {e}"));
                                let _ = fs::copy(&backup, &local_file);
                            } else {
                                info!(project = %encoded, file = %filename, "text-merged (LLM validation failed)");
                                result.text_merged += 1;
                            }
                        }
                    } else {
                        // LLM unavailable — text fallback
                        let fallback = text_fallback_merge(filename, &local_content, &remote_content);
                        if let Err(e) = fs::write(&local_file, &fallback) {
                            result.errors.push(format!("{encoded}/{filename}: fallback write failed: {e}"));
                            let _ = fs::copy(&backup, &local_file);
                        } else {
                            info!(project = %encoded, file = %filename, "text-merged (fallback)");
                            result.text_merged += 1;
                        }
                    }
                }
            }
        }
    }

    // Push merged results back to remote
    if result.has_changes() {
        for encoded in &all_projects {
            let local_mem = sync_cache.join("projects").join(encoded).join("memory");
            if local_mem.exists() {
                if let Err(e) = push_merged_memory(&remote_base, encoded, &sync_cache).await {
                    warn!(project = %encoded, error = %e, "failed to push merged memory");
                    result.errors.push(format!("{encoded}: push failed: {e}"));
                }
            }
        }
        info!(summary = %result.summary(), "memory merge complete");
    }

    // Clean up staging
    let _ = fs::remove_dir_all(&staging);

    result
}

// ---------------------------------------------------------------------------
// LLM merge via Odin
// ---------------------------------------------------------------------------

/// Call Odin's `/v1/chat/completions` to merge two file versions.
async fn llm_merge(
    client: &Client,
    odin_url: &str,
    filename: &str,
    local_content: &str,
    remote_content: &str,
) -> Option<String> {
    let system_prompt = if filename == "MEMORY.md" {
        "You are merging two versions of a MEMORY.md index file from different workstations. \
         Each version contains section headers (## Section) and bullet-point entries.\n\n\
         Rules:\n\
         - Keep ALL unique entries from both versions — do not drop anything\n\
         - Deduplicate entries that refer to the same topic (even if wording differs slightly)\n\
         - Preserve all section headers from both versions\n\
         - If an entry appears in both but with different details, keep the more detailed version\n\
         - Maintain the same markdown format: ## headers with - bullet entries underneath\n\
         - Output ONLY the merged content, no commentary or explanation"
    } else {
        "You are merging two versions of a knowledge memory file from different workstations. \
         Each version may have YAML frontmatter (---name/description/type---) followed by markdown.\n\n\
         Rules:\n\
         - If both have YAML frontmatter, output one frontmatter block with the more descriptive values\n\
         - Merge the markdown body: keep ALL unique facts from both, remove exact duplicates\n\
         - If both state contradictory facts, keep the more specific or recent one\n\
         - Preserve markdown formatting (headers, bold, code blocks, lists)\n\
         - Output ONLY the merged file content, no commentary or explanation"
    };

    let user_prompt = format!(
        "=== VERSION A (local workstation) ===\n{local_content}\n\n\
         === VERSION B (remote workstation) ===\n{remote_content}\n\n\
         === MERGED RESULT ==="
    );

    let payload = serde_json::json!({
        "model": "default",
        "stream": false,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ]
    });

    let resp = match client
        .post(format!("{}/v1/chat/completions", odin_url))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            warn!(status = %r.status(), "Odin merge request failed");
            return None;
        }
        Err(e) => {
            warn!(error = %e, "Odin unreachable for memory merge");
            return None;
        }
    };

    let body: ChatCompletionResponse = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            warn!(error = %e, "failed to parse Odin merge response");
            return None;
        }
    };

    let text = body
        .choices
        .first()?
        .message
        .content
        .trim()
        .to_string();

    // Strip <think> tags (Qwen3 pattern)
    let text = strip_think_tags(&text);

    // Strip markdown fences if the LLM wrapped the output
    let text = strip_markdown_fences(&text);

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Validate merged content: topic files with frontmatter must keep it.
fn validate_merged(original_path: &Path, merged: &str) -> bool {
    // If original starts with "---", merged must too
    if let Ok(original) = fs::read_to_string(original_path) {
        if original.starts_with("---") && !merged.starts_with("---") {
            warn!(path = %original_path.display(), "LLM dropped frontmatter — rejecting");
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Text-based fallback merge (no LLM)
// ---------------------------------------------------------------------------

fn text_fallback_merge(filename: &str, local_content: &str, remote_content: &str) -> String {
    if filename == "MEMORY.md" {
        // Keep local content, append unique lines from remote
        let local_lines: HashSet<&str> = local_content.lines().collect();
        let mut result = local_content.to_string();
        for line in remote_content.lines() {
            if !local_lines.contains(line) {
                result.push('\n');
                result.push_str(line);
            }
        }
        result
    } else {
        // Append unique lines from remote with merge marker
        let local_lines: HashSet<&str> = local_content.lines().collect();
        let mut new_lines = Vec::new();
        for line in remote_content.lines() {
            if !local_lines.contains(line) {
                new_lines.push(line);
            }
        }
        if new_lines.is_empty() {
            local_content.to_string()
        } else {
            format!(
                "{}\n\n<!-- merged from remote -->\n{}",
                local_content.trim_end(),
                new_lines.join("\n")
            )
        }
    }
}

// ---------------------------------------------------------------------------
// File discovery and classification
// ---------------------------------------------------------------------------

fn discover_local_projects(sync_cache: &Path) -> Vec<String> {
    let projects_dir = sync_cache.join("projects");
    let mut projects = Vec::new();
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let mem_dir = entry.path().join("memory");
            if mem_dir.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    projects.push(name.to_string());
                }
            }
        }
    }
    projects
}

async fn discover_remote_projects(remote_base: &str) -> Vec<String> {
    // rsync list remote directories
    let output = tokio::process::Command::new("ssh")
        .args([
            "-o", "ConnectTimeout=3",
            "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=accept-new",
        ])
        .arg(remote_base.split(':').next().unwrap_or(""))
        .arg(format!(
            "ls '{}' 2>/dev/null",
            format!("{}/projects/", remote_base.split(':').nth(1).unwrap_or(""))
        ))
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        }
        _ => Vec::new(),
    }
}

async fn pull_remote_memory(
    remote_base: &str,
    encoded: &str,
    staging: &Path,
) -> Result<(), String> {
    let stage_dir = staging.join(encoded).join("memory");
    fs::create_dir_all(&stage_dir).map_err(|e| e.to_string())?;

    let remote_path = format!("{}/projects/{}/memory/", remote_base, encoded);

    let output = tokio::process::Command::new("rsync")
        .args([
            "--archive",
            "--compress",
            "--checksum",
            "--timeout=5",
            &remote_path,
            &stage_dir.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| format!("rsync failed: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Not an error if remote dir doesn't exist
        if stderr.contains("No such file") || stderr.contains("vanished") {
            Ok(())
        } else {
            Err(format!("rsync exit {}: {}", output.status, stderr.trim()))
        }
    }
}

async fn push_merged_memory(
    remote_base: &str,
    encoded: &str,
    sync_cache: &Path,
) -> Result<(), String> {
    let local_mem = sync_cache
        .join("projects")
        .join(encoded)
        .join("memory/");

    let remote_path = format!("{}/projects/{}/memory/", remote_base, encoded);

    let output = tokio::process::Command::new("rsync")
        .args([
            "--archive",
            "--compress",
            "--checksum",
            "--timeout=5",
            &local_mem.to_string_lossy(),
            &remote_path,
        ])
        .output()
        .await
        .map_err(|e| format!("rsync push failed: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "rsync push exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn collect_md_files(local_dir: &Path, remote_dir: &Path) -> Vec<String> {
    let mut files = HashSet::new();

    for dir in [local_dir, remote_dir] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".md") {
                        files.insert(name.to_string());
                    }
                }
            }
        }
    }

    let mut sorted: Vec<String> = files.into_iter().collect();
    sorted.sort();
    sorted
}

fn classify_file(local: &Path, remote: &Path) -> FileState {
    let local_hash = hash_file(local);
    let remote_hash = hash_file(remote);

    match (local_hash, remote_hash) {
        (Some(lh), Some(rh)) if lh == rh => FileState::Identical,
        (Some(_), Some(_)) => FileState::Diverged,
        (Some(_), None) => FileState::LocalOnly,
        (None, Some(_)) => FileState::RemoteOnly,
        (None, None) => FileState::Identical, // both missing = no-op
    }
}

fn copy_remote_to_local(remote: &Path, local: &Path) -> Result<(), String> {
    if let Some(parent) = local.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(remote, local)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hash_file(path: &Path) -> Option<String> {
    let contents = fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(&contents)))
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
}

fn strip_think_tags(text: &str) -> String {
    // Remove <think>...</think> blocks (Qwen3 pattern)
    let mut result = text.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result.find("</think>") {
            result = format!(
                "{}{}",
                &result[..start],
                &result[end + "</think>".len()..]
            );
        } else {
            // Unclosed <think> tag — remove from <think> to end
            result.truncate(start);
            break;
        }
    }
    result.trim().to_string()
}

fn strip_markdown_fences(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() >= 2
        && lines[0].starts_with("```")
        && lines[lines.len() - 1].starts_with("```")
    {
        lines[1..lines.len() - 1].join("\n")
    } else {
        text.to_string()
    }
}
