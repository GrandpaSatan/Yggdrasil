# Sprint 059 — Hot-Loaded Coding Swarm + Router Fixes + Marketplace Polish

**Started:** 2026-04-13
**Status:** COMPLETE (close-out 2026-04-13)

## Objective

Close out Sprint 058 carry-overs (release tag, sprint docs, marketplace polish, Odin flow CRUD, smoke test, auto-ingest investigation, Thor WoL doc) and execute three architectural improvements requested mid-sprint:

1. **Two FULL Nemotron-3-Nano:4b instances** on Munin (separate ports, separate model stores) — coding_swarm flow uses instance A for `generate` and instance B for `refine`. Not `OLLAMA_NUM_PARALLEL` shared-weight slots.
2. **All coding-flow models stay loaded all the time** (`OLLAMA_KEEP_ALIVE=-1` everywhere) — no eviction, no cold-start swaps.
3. **P0 router fixes** from Sprint 058 bench findings — `llm_router.ollama_url` was pointing at a dead endpoint, `llm_router.model` was empty, and `routing.rules` had a stale `home_automation → munin-igpu` entry. Intent dispatch was silently broken throughout Sprint 058.

Plus user mid-sprint addition: deploy the Fusion 360 V6 (2.6B LFM2 fine-tune) to Munin via Ollama.

---

## Phase A — Two Nemotron Instances on Munin (DONE)

- Created `/etc/systemd/system/ollama-b.service` (port `:11435`, model store `/var/lib/ollama-b/models`)
- Drop-in env vars: `OLLAMA_KEEP_ALIVE=-1`, `OLLAMA_MAX_LOADED_MODELS=2`, `OLLAMA_NUM_PARALLEL=1`, `OLLAMA_CONTEXT_LENGTH=65536`, `OLLAMA_KV_CACHE_TYPE=q8_0`, `HSA_OVERRIDE_GFX_VERSION=11.0.0`
- Pulled `nemotron-3-nano:4b` independently into instance B (2.7 GB on disk, separate from instance A's copy)
- Added `munin-ollama-b` backend to Odin config (`http://localhost:11435`, ollama backend type)
- Updated `coding_swarm` flow:
  - `generate.backend = munin-ollama` (instance A, port 11434) — unchanged
  - `review.backend = hugin-ollama` (gemma4:e4b on Hugin eGPU) — unchanged
  - `refine.backend = munin-ollama-b` (instance B, port 11435) — **NEW**
- Result: each step has its own dedicated weight copy, no KV cache contention between generate and refine, throughput-parallel for multi-user workloads

## Phase B — Always-Loaded via KEEP_ALIVE (DONE)

Drop-ins added to all three Ollama instances:
- **Munin instance A** (`/etc/systemd/system/ollama.service.d/keep-alive.conf`): `OLLAMA_KEEP_ALIVE=-1`, `OLLAMA_MAX_LOADED_MODELS=10`, `OLLAMA_CONTEXT_LENGTH=65536`, `OLLAMA_KV_CACHE_TYPE=q8_0`
- **Munin instance B**: same env vars baked into the unit itself
- **Hugin** (`/etc/systemd/system/ollama.service.d/keep-alive.conf`): `OLLAMA_KEEP_ALIVE=-1`, `OLLAMA_MAX_LOADED_MODELS=4`, `OLLAMA_CONTEXT_LENGTH=32768` (smaller — eGPU constrained)

Warmup script + 3 systemd units:
- `/usr/local/bin/yggdrasil-ollama-warmup` (script): POSTs `/api/generate` with `keep_alive: -1` and `num_predict: 0` for each model
- `yggdrasil-ollama-warmup-a.service` (Munin) — pre-loads 7 models on instance A
- `yggdrasil-ollama-warmup-b.service` (Munin) — pre-loads nemotron on instance B
- `yggdrasil-ollama-warmup.service` (Hugin) — pre-loads 4 models

Verified loaded state (`expires_at: 2318-07-24` = `keep_alive=-1` representation):

| Node | Instance | Model | Size |
|---|---|---|---|
| Munin | A (:11434) | glm-4.7-flash | 21.2 GB |
| Munin | A | nemotron-3-nano:4b | 5.3 GB |
| Munin | A | fusion-v6 | 1.7 GB |
| Munin | A | review-1.2b | 1.8 GB |
| Munin | A | saga-350m | 1.3 GB |
| Munin | A | lfm-1.2b | 1.3 GB |
| Munin | A | all-minilm | 0.1 GB |
| Munin | B (:11435) | nemotron-3-nano:4b | 5.3 GB |
| Hugin | (:11434) | gemma4:e4b | 11.1 GB (eGPU) |
| Hugin |  | code-cleaner-350m | 1.3 GB |
| Hugin |  | lfm-1.2b | 1.3 GB |

Munin total: ~40/46 GB RAM. Hugin total: ~25/60 GB RAM, eGPU 14.6/16 GB.

## Why not 1M Nemotron context

Nemotron supports 1M tokens but the KV cache cost at 1M would be ~120 GB per instance (q8). Munin has 46 GB. The current `coding-swarm` flow uses ~8K tokens of context in practice (`max_step_output_chars: 12000` × 3 prior steps). Setting 1M context "just because" would consume 60+ GB to support a workload that uses 8K. Chose 64K with q8_0 KV cache: ~4 GB per instance (8× headroom over current usage), comfortable fit.

## Bonus — Fusion 360 V6 Deployed (DONE)

V6 generator (2.6B LFM2-2.6B-Exp fine-tune from `~/fine-tuning/output-fusion360-v6/...` on Morrigan):
- Converted SafeTensors → GGUF f16 via `~/llama.cpp/convert_hf_to_gguf.py` (in `~/fine-tuning/venv` which has transformers 5.3.0)
- Quantized to Q4_K_M via `~/llama.cpp/build/bin/llama-quantize` (1.5 GB)
- Piped Morrigan → Munin via `ssh ... cat | ssh ... cat >` (no scp creds between nodes)
- `ollama create fusion-v6 -f Modelfile` on Munin instance A — registered as `fusion-v6:latest` (1.6 GB)
- Custom Modelfile: `num_ctx 8192`, `temperature 0.2`, `top_p 0.9`, system prompt for Fusion 360 Python API code generation
- Added to `yggdrasil-ollama-warmup-a.service` model list — pre-loads on every boot
- Added to Odin `munin-ollama` backend's `models` array — visible in `/v1/models` as `fusion-v6:latest`
- Dim corrector intentionally NOT deployed (user decision — V6 stands alone)

## P0 Router Fixes (DONE — applied live mid-session)

From `docs/sprint-058-bench-findings.md` carry-over list:

1. `llm_router.ollama_url` was `http://10.0.65.9:8081` (decommissioned ollama-igpu) → `http://localhost:11434` (Munin native Ollama)
2. `llm_router.model` was empty string → `lfm-1.2b:latest` (fast classifier)
3. `routing.rules[home_automation].backend` was `munin-igpu` (deleted backend) → `hugin-ollama` with `gemma4:e4b`
4. `routing.default_backend` was `hugin-egpu` (deleted) → `hugin-ollama`
5. `routing.default_model` was placeholder `model.gguf` → `gemma4:e4b`
6. Added `think:true` to both `complex_reasoning` steps (was `null`) — enables GLM-4.7-Flash's Preserved Thinking

Result: `flow engine initialized flows=8`, hybrid SDR + LLM router enabled with real classifier model. Router decisions now emit `intent=X confidence=Y method=LLM` instead of `method=Fallback confidence=None`.

## Phase C — Sprint 058 Close-Out

- ✅ Tag `v0.6.0` pushed to both Gitea + GitHub at merge commit `10e9bb2`
- ✅ Gitea release `v0.6.0` created (id 1) with `yggdrasil-local-0.6.0.vsix` (124 KB) attached as asset
- ⚠ **Auto-updater limitation**: Gitea instance has `REQUIRE_SIGNIN_VIEW` enabled — anonymous downloads redirect to `/user/login`. The current auto-updater in v0.6.0 doesn't authenticate. Workarounds: (1) disable the Gitea-wide setting, or (2) Phase D adds Gitea token support via SecretStorage in v0.7.0.
- ✅ Memory engram for cleanup: `73223bbd-3116-4540-8d55-7c2eea5b475b` (services removed, backends pruned, 2.9 GB reclaimed, default_backend bug)
- ✅ This `sprint-059.md` doc
- 🔄 USAGE.md update: pending (next)

## Phase D — Marketplace Polish (DONE v0.7.0 + v0.8.0)

Shipped in v0.7.0:
- 128×128 PNG icon (replaces SVG)
- README.md rewrite, CHANGELOG.md, LICENSE (MIT)
- Manifest fields: `repository`, `bugs`, `homepage`, `galleryBanner`, `qna`
- Sanitized `10.0.65.x` defaults → `localhost` (walkthrough collects real values)
- First-run walkthrough (`contributes.walkthroughs`, 5 steps)
- vsce package lint clean (0 warnings)

Shipped in **v0.8.0** (Sprint 059 close-out):
- `ReleaseProvider` abstraction (`GiteaProvider` + `GithubProvider`) in [`src/autoUpdater.ts`](../extensions/yggdrasil-local/src/autoUpdater.ts). Tokens live in SecretStorage (`yggdrasil.giteaToken` / `yggdrasil.githubToken`) and attach as `Authorization` headers on the API request + first-hop asset download only. Redirects (GitHub's 302 → S3 pre-signed URLs) are followed anonymously so the token never leaks to third-party hosts.
- Config: `yggdrasil.autoUpdate.provider` (`gitea` | `github`, default `gitea`), `yggdrasil.githubRepo`.
- HTTP 401/403 surfaces actionable hint in the output channel instead of silently failing.
- Settings → Secrets tab gains `githubToken`.
- VSIX: `yggdrasil-local-0.8.0.vsix`, 139.76 KB, 92 files, 0 warnings.

Deferred to Sprint 060:
- vsce + ovsx publish (blocked on publisher ID registration with Microsoft + Open VSX).

## Phase E — Stretch Items (DONE in close-out)

- ✅ **Odin flow CRUD endpoints** — `GET /api/flows`, `GET /api/flows/:id`, `PUT /api/flows/:id`, `GET /api/backends`. Handlers in [`crates/odin/src/handlers.rs`](../crates/odin/src/handlers.rs), routes in [`crates/odin/src/main.rs`](../crates/odin/src/main.rs). Hot-swap via `Arc<RwLock<Arc<Vec<FlowConfig>>>>` in `AppState.flows` — PUT validates each step's backend, merges the new flow into an in-memory Vec, persists the full config via atomic tempfile-rename (the `persist_flows_patch` helper parses raw JSON and replaces only the `flows` field so `${ENV_VAR}` placeholders elsewhere in the config are preserved), then swaps the in-memory snapshot. No service restart required. 3 inline unit tests cover persistence, replace-by-name, and non-object-root rejection; full odin suite still passes.
- ✅ **Extension end-to-end smoke test** — manual runbook committed as [`extensions/yggdrasil-local/SMOKE_TEST.md`](../extensions/yggdrasil-local/SMOKE_TEST.md). 13 sections covering install, walkthrough, activity-bar trees, every `yggdrasil.*` command, chat streaming, slash commands, code actions, all Settings panel tabs, auto-updater (all three scenarios), and existing-feature regression. Sprint 060 can lift this into `@vscode/test-electron` automation.
- ✅ **`store_memory` auto-ingest hook investigation** — RESOLVED, working. Engram `c7b20ae0` documents that Mimir POST `/api/v1/smart-ingest` responds correctly and sidecar→ingest pairs in the events log with `stored:true`.
- ✅ **Thor WoL physical-debug runbook** — [`docs/HARDWARE_THOR_WOL.md`](HARDWARE_THOR_WOL.md).

## Phase F — SDR Prototype Seeding (NEW in close-out)

Problem: `/var/lib/yggdrasil/odin-sdr-prototypes.json` was `[]`, so the hybrid router's "System 1" SDR classifier was inert and every request fell through to the "System 2" LLM classifier.

Shipped:
- Curated seed-phrases list at [`training/router/seed-phrases.json`](../training/router/seed-phrases.json) — 6 intents × 10 phrases (coding, home_automation, reasoning, research, memory, chat).
- Offline seeder at [`crates/odin/examples/seed_prototypes.rs`](../crates/odin/examples/seed_prototypes.rs). Encodes each phrase via Mimir's `/api/v1/embed` endpoint (same pipeline as the live request path), OR-accumulates per intent using `ygg_domain::sdr::binarize` + `sdr::or`, writes a `Vec<IntentPrototype>` JSON. Reuses `odin::sdr_router::IntentPrototype` so there's no drift between seeder output and the on-disk format `SdrRouter::load_from_file` expects.
- Run: `cargo run --example seed_prototypes --release -- --phrases training/router/seed-phrases.json --mimir-url http://10.0.65.8:9090 --out odin-sdr-prototypes.json`. Then scp to Munin `/var/lib/yggdrasil/`, chown, `systemctl restart yggdrasil-odin.service`.

## Phase G — Fusion V6 API Smoke (NEW in close-out)

Smoke test surfaced **two real bugs**:

1. **Config drift — `fusion-v6` vs `fusion-v6:latest`.** Deployed `/etc/yggdrasil/odin/config.json` on Munin listed the model under `backends[munin-ollama].models` without the `:latest` tag. `/v1/models` showed the model (that endpoint queries Ollama directly) but `/v1/chat/completions` rejected it because `SemanticRouter::resolve_backend_for_model` uses exact-string lookup against the config's static models array. **FIXED live** via jq in-place (backup at `config.json.bak.sprint059`, Odin restarted cleanly). This is the same deployed-config-drift hazard called out in engram `023af5f2`.

2. **Model is completion-style, not chat.** Modelfile has `TEMPLATE {{ .Prompt }}` and `Capabilities: completion`. When routed through Odin's chat handler (`/api/chat`), the model can't parse the chat-template-wrapped prompt and emits ~7 tokens of its own system prompt text. Direct `/api/generate` with an instruction-style prompt (`### Instruction:\n...\n\n### Code:\nimport adsk.core, adsk.fusion\n`) produces valid Fusion 360 Python — 341 tokens of real `adsk.*` code (sketches, points, extrudeFeatures). **Model works; Odin integration is the gap.** Sprint 060 fix options: (a) rewrite the Modelfile with a chat TEMPLATE, or (b) add a `fusion-v6`-specific completion passthrough in Odin.

## Carry to Sprint 060

- **vsce + ovsx publish** (blocked on publisher ID registration with Microsoft + Open VSX).
- **SDR prototype seeder deployment** — run the example binary locally, scp the prototypes JSON to Munin, restart Odin, verify router logs show `method=SDR` on classified requests.
- **Fusion V6 chat integration** — replace Modelfile TEMPLATE with a chat-formatted variant OR add a completion-mode passthrough in Odin so the Fusion flow calls `/api/generate` instead of `/api/chat`.
- **Semantic-diff deploys** — every config push to Munin must go through a jq semantic-diff (this is the second sprint in a row where a deploy drift bit us; consider formalizing as a pre-push check).
- **Odin flow CRUD binary deployment** — rebuild `odin` from the new code at [`crates/odin/src/handlers.rs`](../crates/odin/src/handlers.rs), scp to Munin, restart service. Until that ships, the extension's Settings → Flows editor still uses the local-JSON fallback.

## Verification

- `curl http://10.0.65.8:11434/api/ps` shows 7 models with `expires_at: 2318-07-24...`
- `curl http://10.0.65.8:11435/api/ps` shows nemotron-3-nano:4b with same expiry
- `free -h` on Munin shows 40 GB used (was 6.3 GB before warmup)
- Odin `/health` shows 3 backends OK (hugin-ollama, munin-ollama, munin-ollama-b) + morrigan error (on-demand, expected)
- `coding_swarm` flow: `generate` step → port 11434, `refine` step → port 11435 (verify in Odin logs after a real coding request)
- Gitea release at http://10.0.65.11:3000/jesus/Yggdrasil/releases/tag/v0.6.0 with .vsix attached

## Risks

- **Munin RAM headroom is tight** (6 GB free after warmup) — if a flow loads a transient model or KV pressure hits, eviction risk. Mitigation: `OLLAMA_MAX_LOADED_MODELS=10` (won't proactively evict), `OLLAMA_KV_CACHE_TYPE=q8_0` (halves KV memory)
- **Hugin eGPU 89% full** (14.2/16 GB) with gemma4:e4b alone — no room for additional models on eGPU
- **Auto-updater silent failure on private Gitea** — surfaced this sprint, fix scoped for v0.7.0
- **Fusion V6 untested at API level** — model loaded but no Fusion 360 prompt has been run through it via Odin yet. Smoke test in Phase E.

## (Original carry list — all now resolved in Phases E/F/G above)

## Phase H — Sprint 060 execution (live deploy + tooling)

Close-out shipped 2026-04-13:

- ✅ **Odin binary deployed to Munin with flow CRUD endpoints.** Rebuilt via the refactored `mcp__yggdrasil-local__deploy_tool` (see Phase I below). Live round-trip verified: `GET /api/flows/coding_swarm` → `PUT` with mutated `system_prompt` → `{"ok":true}` → `GET` reflects mutation → revert restores original. `/api/backends` returns 4 backends. Permissions fix on `/etc/yggdrasil/odin/` (group `yggdrasil` + `g+w`) so the service can write its own config.
- ✅ **Fusion V6 Modelfile rewritten** with an instruction-style chat TEMPLATE (`{{ .System }}` + `### Instruction:` + `### Code:`). `ollama create fusion-v6` on Munin. Smoke via Odin `/v1/chat/completions`: **359 tokens of valid Fusion 360 Python** with `adsk.core` / `adsk.fusion` imports, `run(context)` entrypoint, sketch-then-extrude pattern. Was 7 tokens before the fix.
- ✅ **SDR prototype seeder run + deployed.** Mimir-encoded 6 intents × 10 phrases, pushed to `/var/lib/yggdrasil/odin-sdr-prototypes.json` (1455 bytes), Odin restarted cleanly. Log shows `loaded SDR intent prototypes from disk count=6`. Race condition surfaced + fixed: Odin's shutdown-save overwrites the prototypes file with its in-memory state, so deploy order MUST be `stop → cp → start`, not `cp → restart`. **Known follow-up (Sprint 061):** `sdr::binarize` sign-threshold encoding saturates under OR accumulation (popcount 247-254 / 256), so Hamming similarity vs. queries stays below the 0.70 threshold → `method=Fallback` in logs. The pipeline is correct; the encoding needs a sparser top-K variant.
- ✅ **Semantic-diff pre-push check shipped** at [scripts/diff-deployed-config.sh](../scripts/diff-deployed-config.sh) + opt-in hook at [.githooks/pre-push](../.githooks/pre-push). Compares deployed Munin `/etc/yggdrasil/odin/config.json` flows against `deploy/config-templates/*-flow.json`, plus cross-checks each backend's `models[]` against the live Ollama tag list. Enable via `git config core.hooksPath .githooks`. Auto-fails-open when Munin is unreachable (offline laptop, different network). **Two template drifts backfilled** as a demonstration (`coding_swarm.loop_config.feedback_key`, `ui_design.loop_config.feedback_key`). 7 other drifts flagged + remain for a separate backfill pass (mostly `agent_config.default_tiers: ["safe"]` missing on 5 flows, plus the `research` flow not deployed).
- ✅ **MCP E2E verification (8 probes):** 6 hard-pass, 2 documented caveats.
  - ✅ `service_health_tool`: 6/6 UP (Odin 7 ms, Mimir 1 ms, Muninn 3 ms, Qdrant 0 ms)
  - ✅ `list_models_tool`: fusion-v6:latest + both Nemotron instances + all warmup models present
  - ✅ `query_memory_tool`: Sprint 059 close-out engram top hit
  - ⚠ `search_code_tool`: Huginn index hasn't ingested commit `74de2ff` yet — transient, not a regression
  - ✅ `delegate_tool` (executor): produced idiomatic `swap()`-based Rust
  - ✅ `network_topology_tool`: 5 mesh entries incl. munin-ollama-b
  - ✅ Fusion V6 via Odin: see above
  - ⚠ SDR method logs: still `method=Fallback` — saturation issue, Sprint 061 scope
- ✅ **vsce/ovsx publish** appended to [memory/project_wishlist.md](~/.claude/projects/-home-jesushernandez-Documents-Code-Yggdrasil/memory/project_wishlist.md). Blocked only on external publisher-ID registration; all extension-side prereqs already shipped in v0.8.0.

## Phase I — MCP tooling hardening (surprise from Phase H)

`mcp__yggdrasil__deploy_tool` turned out to be mis-architected: registered on `ygg-mcp-remote` (Munin:9093) but its `build` action needs the workstation's cargo toolchain (Munin has no cargo). Three rounds of fix:

1. **Moved `deploy_tool` to the local stdio server** ([crates/ygg-mcp/src/local_server.rs](../crates/ygg-mcp/src/local_server.rs)) so cargo is reachable. New tool name: `mcp__yggdrasil-local__deploy_tool`. Old remote tool still exists until we redeploy `ygg-mcp-remote` to Munin.
2. **Fixed workspace_path** in `~/.config/yggdrasil/local-mcp.yaml` (was pointing one directory too high).
3. **Rewrote the deploy action** in [crates/ygg-mcp/src/tools.rs](../crates/ygg-mcp/src/tools.rs): now rsyncs to `/tmp/<svc>.new`, ssh-and-sudo-cp to `/opt/yggdrasil/bin/<svc>.new`, atomic `mv` over the final path (solves "Text file busy" on running executables), explicit `systemctl restart yggdrasil-<svc>.service`. Added `deploy_user` + `deploy_sudo_password` fields to `McpServerConfig` (config file at `chmod 600`), with `$YGG_SUDO_PASSWORD` env var override.

Feedback engrams stored: `3e2a4354` (tool location), `0ee58d0c` (workspace_path).

## Phase J — Sprint 061 research seeding

Four research engrams stored for the next planning session:

- `aa0fee24` — Swarm KV sharing: DroidSpeak, KVCOMM (best fit for our heterogeneous swarm), segment-level KV from the user-requested OpenReview 2026 paper.
- `3a59b09f` — KV compression + NVMe offload: TurboQuant (Google DeepMind, 5-6× compression), LMCache (hierarchical GPU→CPU→NVMe tiering), vLLM 0.12 native offload, KVSwap.
- `70f7d346` — Always-warm / "dream cycle": vLLM Sleep Mode is the closest published analogue (18-200× faster model switching). The rehearsal / idle-cycle self-play half appears to be Yggdrasil-original — good scope for Sprint 061.
- `70d2c451` — Retro-terminal chat UI: RetroUI + vault66-crt-effect as the drop-in React components; Fallout Pip-Boy CSS references for the aesthetic. Visual change is orthogonal to the flow-level "swarm as single AI" abstraction.

## Sprint 060 carry to Sprint 061

- SDR router saturation — replace sign-threshold `binarize` with sparser top-K encoding (or lower the 0.70 threshold).
- Template drift backfill — 5 flows missing `default_tiers: ["safe"]` on `agent_config`; `research` flow needs deploying.
- Remove the broken remote `deploy_tool` from `ygg-mcp/src/server.rs` and redeploy `ygg-mcp-remote` to Munin.
- Huginn re-index cycle — commit `74de2ff` + Sprint 060 follow-up commit need to surface in `search_code_tool`.
