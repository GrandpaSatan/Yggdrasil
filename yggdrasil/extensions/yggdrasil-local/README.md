# Yggdrasil — VS Code Extension

A control panel and chat client for **Yggdrasil**, a self-hosted local-AI ecosystem. Wraps an Ollama-backed model fleet behind a unified router (Odin), an engram memory service (Mimir), and an OpenAI-compatible chat API. This extension gives you the editor surface for all of it.

> **Heads up:** Defaults assume an Odin server on `localhost:8080`. The walkthrough on first install asks for your real endpoints. Yggdrasil itself is open-source — see the homepage for a self-host guide.

## Features

### Activity Bar Sidebar
- **Flows tree** — every flow your Odin instance exposes, grouped by Architecture / Coding / Existing. Click to open the full-width Flows Explorer focused on that flow.
- **Models tree** — live list of models from `GET /v1/models`, grouped by backend, with a `loaded` indicator pulled from `/api/ps`. Auto-refreshes every 30 s.

### Flows Explorer  (`Ctrl+Shift+Y`)
A full-width WebviewPanel that mirrors `sprint-058-flows.html` — topology diagram, AI distribution map (which model on which node), and a tab per flow with:
- Animated SVG flow chart (user → step1 → step2 → response, with loop arcs for iterative flows like `coding_swarm`)
- Step-assignment table: backend, model, role badge
- **System-prompt disclosures per step** — click "view prompt" to see the exact `system_prompt`, `input_template`, and `temperature` each step uses

### Chat Panel  (`Ctrl+Shift+I`)
Continue/Cline-style streaming chat over Odin's OpenAI-compatible `/v1/chat/completions`:
- Model picker (populated from `/v1/models`, grouped by backend, `●` indicator for loaded)
- Flow picker — pin a flow for the current turn (`/flow coding_swarm` shorthand)
- Attachment chips — attach the current file or selection as context
- Markdown rendering with code-fence copy buttons
- Threaded history (up to 50 threads in `globalState`)
- Stop button cancels in-flight stream
- Slash commands: `/flow`, `/model`, `/memory`, `/clear`, `/help`

### Settings Panel  (palette: `Yggdrasil: Open Settings`)
Four tabs that consolidate every Yggdrasil knob into one place:
1. **Endpoints** — Odin / Mimir / Hugin / Gitea URLs with health-probe buttons
2. **Flows** — pick any flow, edit each step's model / system prompt / input template / temperature / max_tokens / loop config; saves via `PUT /api/flows/:id` to Odin (gracefully falls back to local JSON read when that endpoint isn't deployed yet)
3. **Notifications & Hooks** — event-type filter, sound toggle, "Reinstall hooks" button
4. **Secrets** — Gitea token, Home Assistant token, Brave Search key — stored in VS Code's `SecretStorage` (OS keychain-backed: macOS Keychain, Windows Credential Vault, Linux libsecret)

### Editor Code Actions (right-click → `yggdrasil` group)
- **Yggdrasil: Explain Selection** (`Ctrl+Shift+E`) — opens chat seeded with the selection + an explain prompt
- **Yggdrasil: Edit With Model** — prompts you for an instruction, then opens chat with the selection + edit prompt + `coding_swarm` flow hint
- **Yggdrasil: Ask About This File** — opens chat with the active file as an attachment chip

### Memory Dashboard  (`Ctrl+Shift+M`)
Webview panel with session stats — recalled / stored / errors / sidecar engrams, plus an event timeline of the last 20 memory operations. Driven by JSONL events emitted by the bundled `ygg-memory.sh` Claude Code hooks.

### Status Bar
`$(database) Ygg: N recalled · N stored` — click to open the dashboard. Color reflects MCP/Mimir reachability (green/yellow/red).

### Auto-Updater
On startup (max once per hour) the extension checks `${yggdrasil.giteaUrl}/api/v1/repos/${yggdrasil.giteaRepo}/releases/latest` and downloads + installs the attached `.vsix` if newer than installed. Disable with `yggdrasil.autoUpdate.enabled: false`.

## Quick Start

1. Install the `.vsix`:
   ```bash
   code --install-extension yggdrasil-local-*.vsix
   ```
2. Reload VS Code (`Ctrl+Shift+P` → `Developer: Reload Window`).
3. The walkthrough runs on first activation — enter your Odin URL, test connectivity, pick a default flow, open the chat.

If you don't have a Yggdrasil server running, see the [Yggdrasil README](https://github.com/GrandpaSatan/Yggdrasil) for the self-host setup (Munin + Hugin nodes, Odin router, Mimir memory).

## Configuration

Every setting is editable via the Settings panel UI or directly in `settings.json`:

| Setting | Default | What it does |
|---|---|---|
| `yggdrasil.odinUrl` | `http://localhost:8080` | Odin router (chat, models, flow CRUD) |
| `yggdrasil.mimirUrl` | `http://localhost:9090` | Mimir engram memory service |
| `yggdrasil.huginUrl` | `http://localhost:11434` | Direct Ollama (reviewer/vision node) |
| `yggdrasil.giteaUrl` | `http://localhost:3000` | Gitea — auto-updater source |
| `yggdrasil.giteaRepo` | `you/Yggdrasil` | Gitea repo `owner/name` |
| `yggdrasil.autoUpdate.enabled` | `true` | Hourly check for new `.vsix` |
| `yggdrasil.hooks.managed` | `true` | Auto-install Claude Code hooks in `~/.claude/settings.json` |
| `yggdrasil.eventsFile` | `/tmp/ygg-hooks/memory-events.jsonl` | JSONL stream from `ygg-memory.sh` hooks |
| `yggdrasil.notifications.enabled` | `true` | Show toasts on memory events |
| `yggdrasil.notifications.sound` | `false` | Play `paplay` audio cue on store |
| `yggdrasil.notifications.events` | `["ingest", "error"]` | Which event types trigger toasts |

## Architecture

```
                  ┌──────────────────────────┐
   user ────►     │   VS Code Extension      │
                  │                          │
                  │  • Chat Panel  ─────┐    │
                  │  • Flows Panel      │    │
                  │  • Settings Panel   │    │
                  │  • Models Tree      │    │
                  └─────────────────────│────┘
                                        │ HTTP (SSE for chat)
                                        ▼
                  ┌──────────────────────────┐
                  │   Odin Router            │  ← intent classification + flow dispatch
                  │   (Rust, port 8080)      │
                  └────────┬─────────────────┘
                           │
       ┌───────────────────┼─────────────────────┐
       ▼                   ▼                     ▼
   Munin Ollama      Hugin Ollama           Morrigan
   (coder, glm,      (reviewer +            (on-demand
    fusion-v6, ...)   vision)                inference VM)
```

The extension speaks plain HTTP; you can point it at any OpenAI-compatible server, but the Flows / Settings panels assume Odin's `/api/flows` schema.

## Known Limitations

- **Auto-updater + private Gitea**: if your Gitea instance has `REQUIRE_SIGNIN_VIEW` enabled, the unauthenticated `/releases/latest` check returns null and updates won't be picked up. Workaround in v0.7.0 will add optional Gitea-token auth via `SecretStorage`. For now: install the new `.vsix` manually, or disable the Gitea-wide signin requirement.
- **Settings panel "Save Flow"** posts to `PUT /api/flows/:id` on Odin. If your Odin build doesn't have those endpoints yet (Sprint 058 didn't ship them), the save toast says so — flow viewing still works (read-only fallback to `deploy/config-templates/*.json` from the workspace).

## Development

```bash
cd extensions/yggdrasil-local
npm install
npm run watch       # auto-recompile on save
```

After changes, repackage:
```bash
npm run compile
npx @vscode/vsce package --no-dependencies
code --install-extension yggdrasil-local-*.vsix --force
```

Open with `Ctrl+Shift+P` → `Developer: Reload Window` to pick up the new build.

## License

MIT — see [LICENSE](LICENSE).

## Issues

Report bugs at the [Yggdrasil issue tracker](https://github.com/GrandpaSatan/Yggdrasil/issues).
