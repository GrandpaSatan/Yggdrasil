# Yggdrasil Usage Guide

## System Overview

Yggdrasil is a centralized configuration management system with cross-platform support (Linux/Windows) and multi-workstation memory consolidation.

## Installation

### Full install (first time on a node)

```bash
cd ~/yggdrasil
deploy/install.sh munin   # installs odin + mimir + systemd units + configs
deploy/install.sh hugin   # installs huginn + muninn + SSHFS mount
```

### Rollback

```bash
deploy/rollback.sh munin odin  # restores odin.prev binary and restarts
```

## Starting Services

### Start all services on Munin

```bash
ssh your-user@munin
sudo systemctl start yggdrasil-mimir
sudo systemctl start yggdrasil-odin
```

## Sprint Lifecycle

### Starting a sprint

1. Create `/sprints/sprint-NNN.md` with the full sprint plan.
2. Call `sync_docs_tool(event: "sprint_start", sprint_id: "NNN", sprint_content: <full plan>)` — this updates USAGE.md and checks /docs/ invariants.

### Ending a sprint

1. Call `sync_docs_tool(event: "sprint_end", sprint_id: "NNN", sprint_content: <full plan>)` — this:
   - Generates a condensed summary via Qwen3-Coder
   - Archives to Mimir with tags `["sprint", "project:yggdrasil"]`
   - Appends architecture delta to ARCHITECTURE.md
   - Deletes the sprint file
2. Verify `/sprints/` is empty (ready for next sprint).

## Memory Management

### After Meaningful Work
- `store_memory_tool` — persist decisions, schemas, gotchas, next steps

## Configuration Management

- Centralized configuration stored on Munin node
- Workstations maintain local symlinks to central configuration
- rsync ensures consistency between nodes

## Cross-Platform Support

- Unified configuration management for Linux/Windows
- Platform-specific handlers for consistent behavior