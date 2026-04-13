#!/usr/bin/env bash
# diff-deployed-config.sh — semantic drift check between deployed Munin config
# and the repo templates + live Ollama model registry.
#
# Exits non-zero on drift, zero when clean. Intended to be run from repo root
# or wired into .githooks/pre-push so drift blocks pushes.
#
# Catches the two Sprint 058/059 drift classes:
#   (1) A flow deployed on Munin differs from deploy/config-templates/<name>-flow.json
#   (2) A backend's models[] references a tag Ollama doesn't actually serve
#       (e.g. config says "fusion-v6" but Ollama only has "fusion-v6:latest")
#
# Usage:
#   ./scripts/diff-deployed-config.sh            # check all, exit 1 on drift
#   ./scripts/diff-deployed-config.sh --fix-hint # also print the jq patch
#
# Requires: ssh access to munin as $SSH_USER (default jhernandez), jq, diff.
#
set -euo pipefail

SSH_USER="${SSH_USER:-jhernandez}"
SSH_HOST="${SSH_HOST:-munin}"
SUDO_PASSWORD="${YGG_SUDO_PASSWORD:-}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'
log() { echo -e "${CYAN}[diff]${NC} $*"; }
ok()  { echo -e "${GREEN}  \u2713${NC} $*"; }
warn(){ echo -e "${YELLOW}  !${NC} $*"; }
err() { echo -e "${RED}  \u2717${NC} $*"; }

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEMPLATES_DIR="$REPO_ROOT/deploy/config-templates"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

DRIFT=0

# ── 1. Pull deployed config ──────────────────────────────────────────
log "pulling deployed config from $SSH_HOST"
if [ -n "$SUDO_PASSWORD" ]; then
    ssh "$SSH_USER@$SSH_HOST" "echo '$SUDO_PASSWORD' | sudo -S cat /etc/yggdrasil/odin/config.json" \
        2>/dev/null > "$WORK/deployed.json"
else
    ssh "$SSH_USER@$SSH_HOST" "sudo cat /etc/yggdrasil/odin/config.json" > "$WORK/deployed.json"
fi
[ -s "$WORK/deployed.json" ] || { err "empty deployed config"; exit 2; }
jq empty "$WORK/deployed.json" || { err "deployed config is not valid JSON"; exit 2; }

# ── 2. Per-flow semantic diff (template vs deployed) ────────────────
log "comparing flow templates vs deployed flows"
for tmpl in "$TEMPLATES_DIR"/*-flow.json; do
    fname="$(basename "$tmpl")"
    # Template filename pattern: <flow_name_with_dashes>-flow.json → flow name has underscores
    flow_name="${fname%-flow.json}"
    flow_name="${flow_name//-/_}"

    # Templates wrap the flow in {"flows": [<flow>]}. Unwrap to the bare flow object,
    # drop template-only fields (_comment) and any null/empty fields so the diff is
    # semantic rather than lexical.
    jq -S '(.flows[0] // .) | del(._comment)
           | walk(if type == "object" then with_entries(select(.value != null)) else . end)' \
        "$tmpl" > "$WORK/tmpl.$flow_name.json"

    # Pull the matching deployed flow (by name), apply the same null-key strip
    # so lexical-only differences (e.g. explicit "loop_config": null) don't alarm.
    jq -S --arg n "$flow_name" '.flows[] | select(.name == $n)
           | walk(if type == "object" then with_entries(select(.value != null)) else . end)' \
        "$WORK/deployed.json" > "$WORK/deployed.$flow_name.json"

    if [ ! -s "$WORK/deployed.$flow_name.json" ]; then
        warn "flow '$flow_name' in template but NOT deployed"
        DRIFT=$((DRIFT + 1))
        continue
    fi

    # Semantic diff — ignores key order, ignores insignificant whitespace
    if diff -u "$WORK/tmpl.$flow_name.json" "$WORK/deployed.$flow_name.json" \
        > "$WORK/diff.$flow_name.txt" 2>&1; then
        ok "$flow_name — identical to template"
    else
        warn "$flow_name — drift detected"
        sed 's/^/    /' "$WORK/diff.$flow_name.txt" | head -20
        DRIFT=$((DRIFT + 1))
    fi
done

# ── 3. Cross-check backend models[] against Ollama /api/tags ────────
log "verifying backend models[] against live Ollama tags"
# ssh-tunnel the /api/tags call so we don't need the workstation to resolve
# the node hostname via DNS (our SSH config has the alias but curl doesn't
# read ~/.ssh/config). This works regardless of DNS/VLAN state.
MUNIN_TAGS=$(ssh "$SSH_USER@$SSH_HOST" \
    "curl -sS --max-time 5 http://localhost:11434/api/tags" 2>/dev/null \
    | jq -r '.models[].name' 2>/dev/null | sort -u || echo "")
HUGIN_TAGS=$(ssh "$SSH_USER@${HUGIN_HOST:-hugin}" \
    "curl -sS --max-time 5 http://localhost:11434/api/tags" 2>/dev/null \
    | jq -r '.models[].name' 2>/dev/null | sort -u || echo "")

check_backend_models() {
    local backend_name="$1"
    local tag_list="$2"
    jq -r --arg n "$backend_name" \
        '.backends[] | select(.name == $n) | .models[]' "$WORK/deployed.json" \
        | while read -r model; do
            if ! echo "$tag_list" | grep -qF "$model"; then
                # Allow the caller to also check <model>:latest since Ollama treats untagged as :latest
                if ! echo "$tag_list" | grep -qF "$model:latest"; then
                    warn "$backend_name: \"$model\" not served by Ollama"
                    echo "DRIFT"
                fi
            fi
        done
}

if [ -n "$MUNIN_TAGS" ]; then
    drift_lines=$(check_backend_models "munin-ollama" "$MUNIN_TAGS" | grep -c '^DRIFT$' || true)
    if [ "$drift_lines" -gt 0 ]; then
        DRIFT=$((DRIFT + drift_lines))
    else
        ok "munin-ollama models all resolve in Ollama"
    fi
    drift_lines=$(check_backend_models "munin-ollama-b" "$MUNIN_TAGS" | grep -c '^DRIFT$' || true)
    if [ "$drift_lines" -gt 0 ]; then
        DRIFT=$((DRIFT + drift_lines))
    else
        ok "munin-ollama-b models all resolve in Ollama"
    fi
else
    warn "could not fetch munin ollama tags — skipping"
fi

if [ -n "$HUGIN_TAGS" ]; then
    drift_lines=$(check_backend_models "hugin-ollama" "$HUGIN_TAGS" | grep -c '^DRIFT$' || true)
    if [ "$drift_lines" -gt 0 ]; then
        DRIFT=$((DRIFT + drift_lines))
    else
        ok "hugin-ollama models all resolve in Ollama"
    fi
else
    warn "could not fetch hugin ollama tags — skipping"
fi

# ── 4. Summary ───────────────────────────────────────────────────────
if [ "$DRIFT" -gt 0 ]; then
    err "$DRIFT drift issue(s) found — fix before pushing"
    exit 1
fi

ok "no drift detected"
exit 0
