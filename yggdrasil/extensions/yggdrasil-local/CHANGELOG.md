# Changelog

All notable changes to the Yggdrasil VS Code extension are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.0] — 2026-04-13 (Sprint 059)

### Added
- **First-run walkthrough** via `contributes.walkthroughs` — guides new users through Odin URL setup, health check, and first chat.
- **128×128 PNG icon** for marketplace + activity bar.
- Manifest fields for marketplace submission: `repository`, `bugs`, `homepage`, `galleryBanner`, `icon`.
- LICENSE (MIT).
- README rewritten as marketplace-quality documentation.

### Changed
- **Defaults sanitized** — all `10.0.65.x` homelab IPs in default settings replaced with `localhost` placeholders. The walkthrough collects real values on first run.

### Known Issues
- Auto-updater does not yet authenticate to Gitea instances with `REQUIRE_SIGNIN_VIEW` enabled. Tracked for v0.8.0.

## [0.6.0] — 2026-04-13 (Sprint 058)

### Added
- **Activity bar entry** with two tree views — Flows tree (13 flows grouped by Architecture / Coding / Existing) and Models tree (live state from Odin's `/v1/models`, auto-refresh every 30 s).
- **Flows Explorer** (`Ctrl+Shift+Y`) — full-width WebviewPanel ported from `sprint-058-flows.html`. Topology diagram, AI distribution map, per-flow tabs with animated SVG flowcharts and **system-prompt disclosures per step** (click to expand the exact prompt + input template + temperature each model receives).
- **Chat Panel** (`Ctrl+Shift+I`) — Continue/Cline-style streaming chat over Odin's OpenAI-compatible endpoint. Model picker, flow picker, attachment chips (file/selection), markdown renderer, threaded history (max 50 in `globalState`), stop button.
- **Settings Panel** with 4 tabs:
  - Endpoints (with health-probe buttons)
  - Flows (per-step editor: model picker, system prompt textarea, input template, temperature, max_tokens, loop_config — saves to Odin via `PUT /api/flows/:id`, falls back to read-only local-JSON viewing if the endpoint is missing)
  - Notifications & Hooks (event filter, sound toggle, reinstall hooks button)
  - Secrets (Gitea token, HA token, Brave Search key — stored via SecretStorage / OS keychain)
- **Slash commands** in chat: `/flow <name> <msg>`, `/model <id> <msg>`, `/memory <query>`, `/clear`, `/help`.
- **Editor code actions** under right-click "yggdrasil" group:
  - `Yggdrasil: Explain Selection` (`Ctrl+Shift+E`)
  - `Yggdrasil: Edit With Model` (prompts for instruction, applies via chat with `coding_swarm` flow hint)
  - `Yggdrasil: Ask About This File` (attaches current file as context)
- **OdinClient** — typed HTTP client with SSE streaming for chat completions, model/flow/memory queries, health probe, graceful local-JSON fallback for flow CRUD when Odin endpoints aren't deployed.
- New keybindings: `Ctrl+Shift+Y` (flows), `Ctrl+Shift+I` (chat), `Ctrl+Shift+E` (explain selection).

### Changed
- Extension renamed from "Yggdrasil Local" to "Yggdrasil" (broader scope).
- Activity bar icon from monochrome SVG of the Yggdrasil world-tree.
- Categories expanded to `["AI", "Chat", "Machine Learning", "Other"]`.
- Keywords added for marketplace search: `ai`, `llm`, `ollama`, `local-ai`, `homelab`, `chat`, `coding-agent`, `rag`, `mcp`, `yggdrasil`.

## [0.3.0] — 2026-03-28 (Sprint 050)

### Added
- Self-managing extension: bundled `ygg-memory.sh` sidecar script auto-deploys to `~/.yggdrasil/`; `~/.claude/settings.json` hooks auto-installed and pointed at the deployed script.
- Auto-updater: hourly check of Gitea releases; downloads + installs newer `.vsix` automatically.
- Hook manager — health check (script deployed + hooks correct + Mimir reachable), reflected in status-bar color.

### Changed
- Replaced the Rust `ygg-mcp-server` binary with a Node.js MCP server living inside the extension (stdio transport, serves `sync_docs_tool` + `screenshot_tool`).

## [0.2.0] — 2026-04-09

### Added
- `sidecar` event type — emitted by `ygg-memory.sh` when the saga classifier runs.
- Recall events now display the query text instead of the source filename in dashboard + notifications.
- Multi-GPU training support in adjacent training scripts (not part of extension itself).

## [0.1.0] — 2026-03-26 (Sprint 048)

### Added
- Initial release — status bar, output channel, notifications, memory dashboard webview, JSONL event watcher.
