#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════
# memory-merge.sh — SessionStart hook: LLM-based memory merge
# ═══════════════════════════════════════════════════════════════════════
#
# Pulls remote memory from Munin, detects diverged files via SHA256,
# and uses Ollama to intelligently merge both versions before Claude
# reads them. Ensures no memory is lost when working across workstations.
#
# NEVER exits non-zero — hook failures must not block session start.
# ═══════════════════════════════════════════════════════════════════════

# Safety: never block session start
trap 'exit 0' ERR
set -uo pipefail

# ── Source hook env (created by ygg-hooks-init.sh) ────────────────────
[ -f /tmp/ygg-hooks/env ] && . /tmp/ygg-hooks/env

# ── Constants ─────────────────────────────────────────────────────────
MUNIN_IP="${MUNIN_IP:-10.0.65.9}"
REMOTE_USER="${DEPLOY_USER:-jhernandez}"
REMOTE_BASE="/opt/yggdrasil/claude-config"
OLLAMA_URL="http://${MUNIN_IP}:11434"
SYNC_CACHE="$HOME/.claude/.sync-cache"
STAGING="/tmp/ygg-hooks/merge-staging"
MERGE_LOG="/tmp/ygg-hooks/merge.log"
SSH_OPTS="-o ConnectTimeout=3 -o BatchMode=yes -o StrictHostKeyChecking=accept-new"
RSYNC_OPTS="--archive --compress --checksum --timeout=5"

# Models in preference order (fast/small first)
PREFERRED_MODELS=("qwen3:4b" "qwen3.5:4b" "saga:0.6b")
SELECTED_MODEL=""

# Timing
DEADLINE=$(( $(date +%s) + 13 ))
MERGE_COUNT=0
COPY_COUNT=0
FALLBACK_COUNT=0

log() { echo "[merge] $(date +%H:%M:%S) $1" >> "$MERGE_LOG"; }

# ── Phase 1: Connectivity ────────────────────────────────────────────
log "Starting memory merge..."

if ! ssh $SSH_OPTS "$REMOTE_USER@$MUNIN_IP" true 2>/dev/null; then
    log "Munin unreachable — skipping merge"
    exit 0
fi
log "Munin reachable"

mkdir -p "$STAGING"

# ── Phase 2: Pull remote memory to staging ────────────────────────────
# Discover local project memory directories
LOCAL_PROJECTS=()
if [[ -d "$SYNC_CACHE/projects" ]]; then
    for d in "$SYNC_CACHE"/projects/*/memory/; do
        [[ -d "$d" ]] || continue
        encoded=$(basename "$(dirname "$d")")
        LOCAL_PROJECTS+=("$encoded")
    done
fi

# Also discover remote-only projects
REMOTE_PROJECTS=$(ssh $SSH_OPTS "$REMOTE_USER@$MUNIN_IP" \
    "ls '$REMOTE_BASE/projects/' 2>/dev/null" 2>/dev/null || true)

# Build unique project list
declare -A ALL_PROJECTS
for p in "${LOCAL_PROJECTS[@]}"; do ALL_PROJECTS["$p"]=1; done
for p in $REMOTE_PROJECTS; do ALL_PROJECTS["$p"]=1; done

# Pull each project's memory to staging
for encoded in "${!ALL_PROJECTS[@]}"; do
    remote_mem="$REMOTE_BASE/projects/$encoded/memory/"
    stage_mem="$STAGING/$encoded/memory/"
    mkdir -p "$stage_mem"
    rsync $RSYNC_OPTS \
        "$REMOTE_USER@$MUNIN_IP:$remote_mem" "$stage_mem" 2>/dev/null || true
done
log "Pulled ${#ALL_PROJECTS[@]} project memories to staging"

# ── Phase 3: Diff detection ──────────────────────────────────────────
# Arrays to hold files needing merge
DIVERGED_FILES=()   # "encoded|filename" pairs
DIVERGED_LOCAL=()   # local content paths
DIVERGED_REMOTE=()  # remote content paths

for encoded in "${!ALL_PROJECTS[@]}"; do
    local_mem="$SYNC_CACHE/projects/$encoded/memory"
    stage_mem="$STAGING/$encoded/memory"

    # Collect all .md filenames from both sides
    declare -A ALL_FILES
    for f in "$local_mem"/*.md "$stage_mem"/*.md; do
        [[ -f "$f" ]] || continue
        ALL_FILES["$(basename "$f")"]=1
    done

    for filename in "${!ALL_FILES[@]}"; do
        local_file="$local_mem/$filename"
        remote_file="$stage_mem/$filename"

        local_hash="missing"
        remote_hash="missing"
        [[ -f "$local_file" ]] && local_hash=$(sha256sum "$local_file" | awk '{print $1}')
        [[ -f "$remote_file" ]] && remote_hash=$(sha256sum "$remote_file" | awk '{print $1}')

        if [[ "$local_hash" == "$remote_hash" ]]; then
            continue  # identical
        elif [[ "$local_hash" == "missing" ]]; then
            # Remote-only: copy to local
            mkdir -p "$local_mem"
            cp "$remote_file" "$local_file"
            ((COPY_COUNT++))
            log "Copied remote-only: $encoded/$filename"
        elif [[ "$remote_hash" == "missing" ]]; then
            continue  # local-only, will propagate on next sync push
        else
            # Diverged: queue for merge
            DIVERGED_FILES+=("$encoded|$filename")
            DIVERGED_LOCAL+=("$local_file")
            DIVERGED_REMOTE+=("$remote_file")
            log "Diverged: $encoded/$filename"
        fi
    done
    unset ALL_FILES
done

# If nothing diverged, we're done
if [[ ${#DIVERGED_FILES[@]} -eq 0 ]]; then
    log "No diverged files. Copied $COPY_COUNT remote-only files."
    if [[ $COPY_COUNT -gt 0 ]]; then
        printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":%s}}\n' \
            "$(echo "Memory sync: $COPY_COUNT new files copied from remote workstation." | jq -Rs .)"
    fi
    exit 0
fi

log "${#DIVERGED_FILES[@]} files need merging"

# ── Phase 4: Model selection ─────────────────────────────────────────
select_model() {
    local tags
    tags=$(curl --silent --max-time 2 "${OLLAMA_URL}/api/tags" 2>/dev/null) || return 1
    for m in "${PREFERRED_MODELS[@]}"; do
        if echo "$tags" | jq -e --arg m "$m" '.models[]? | select(.name | startswith($m))' &>/dev/null; then
            SELECTED_MODEL="$m"
            return 0
        fi
    done
    return 1
}

if select_model; then
    log "Selected model: $SELECTED_MODEL"
else
    log "No model available — will use text-based fallback"
fi

# ── Phase 5: Merge functions ─────────────────────────────────────────

# LLM merge via Ollama
ollama_merge() {
    local prompt="$1"
    local response
    response=$(curl --silent --max-time 10 \
        -H "Content-Type: application/json" \
        -d "$(jq -n \
            --arg model "$SELECTED_MODEL" \
            --arg prompt "$prompt" \
            '{model: $model, prompt: $prompt, stream: false, options: {temperature: 0.1, num_predict: 4096}}')" \
        "${OLLAMA_URL}/api/generate" 2>/dev/null) || return 1

    # Extract response, strip any <think> tags (Qwen3 pattern)
    local text
    text=$(echo "$response" | jq -r '.response // empty' 2>/dev/null)
    text=$(echo "$text" | sed 's/<think>.*<\/think>//g; s/<think>//g; s/<\/think>//g')

    # Strip leading/trailing whitespace and any markdown fences
    text=$(echo "$text" | sed '/^```/d' | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')

    [[ -n "$text" ]] && echo "$text" || return 1
}

# Build merge prompt based on file type
build_prompt() {
    local filename="$1" local_content="$2" remote_content="$3"

    if [[ "$filename" == "MEMORY.md" ]]; then
        cat <<PROMPT
You are merging two versions of a MEMORY.md index file from different workstations. Each version contains section headers (## Section) and bullet-point entries that link to topic files.

Rules:
- Keep ALL unique entries from both versions — do not drop anything
- Deduplicate entries that refer to the same topic (even if wording differs slightly)
- Preserve all section headers from both versions
- If an entry appears in both but with different details, keep the more detailed version
- Maintain the same markdown format: ## headers with - bullet entries underneath
- Output ONLY the merged MEMORY.md content, no commentary

=== VERSION A (local workstation) ===
${local_content}

=== VERSION B (remote workstation) ===
${remote_content}

=== MERGED RESULT ===
PROMPT
    else
        cat <<PROMPT
You are merging two versions of a knowledge memory file from different workstations. Each version may have YAML frontmatter (---name/description/type---) followed by markdown content.

Rules:
- If both have YAML frontmatter, output one frontmatter block using the more descriptive values
- Merge the markdown body: keep ALL unique facts from both, remove exact duplicates
- If both versions state contradictory facts, keep the one that appears more specific or recent
- Preserve markdown formatting (headers, bold, code blocks, lists)
- Output ONLY the merged file content, no commentary

=== VERSION A (local workstation) ===
${local_content}

=== VERSION B (remote workstation) ===
${remote_content}

=== MERGED RESULT ===
PROMPT
    fi
}

# Text-based fallback merge (no LLM)
text_fallback_merge() {
    local filename="$1" local_file="$2" remote_file="$3"

    if [[ "$filename" == "MEMORY.md" ]]; then
        # Line-based dedup, preserving order from local first
        local tmp
        tmp=$(mktemp)
        # Keep local content, then append unique lines from remote
        cp "$local_file" "$tmp"
        while IFS= read -r line; do
            if ! grep -qFx "$line" "$tmp" 2>/dev/null; then
                echo "$line" >> "$tmp"
            fi
        done < "$remote_file"
        cat "$tmp"
        rm -f "$tmp"
    else
        # Append unique lines from remote with merge marker
        local tmp
        tmp=$(mktemp)
        cp "$local_file" "$tmp"
        local new_lines
        new_lines=$(comm -23 <(sort "$remote_file") <(sort "$local_file") 2>/dev/null || true)
        if [[ -n "$new_lines" ]]; then
            echo "" >> "$tmp"
            echo "<!-- merged from remote on $(date -u +%Y-%m-%dT%H:%M:%SZ) -->" >> "$tmp"
            echo "$new_lines" >> "$tmp"
        fi
        cat "$tmp"
        rm -f "$tmp"
    fi
}

# ── Phase 6: Execute merges ──────────────────────────────────────────
for i in "${!DIVERGED_FILES[@]}"; do
    # Check time budget
    if [[ $(date +%s) -ge $DEADLINE ]]; then
        log "Time budget exhausted — skipping remaining ${#DIVERGED_FILES[@]} - $i merges"
        break
    fi

    IFS='|' read -r encoded filename <<< "${DIVERGED_FILES[$i]}"
    local_file="${DIVERGED_LOCAL[$i]}"
    remote_file="${DIVERGED_REMOTE[$i]}"

    local_content=$(cat "$local_file")
    remote_content=$(cat "$remote_file")

    # Skip if either is too large for LLM (>8KB)
    local_size=$(wc -c < "$local_file")
    remote_size=$(wc -c < "$remote_file")

    # Back up local before any modification
    cp "$local_file" "${local_file}.pre-merge"

    merged=""
    used_llm=false

    if [[ -n "$SELECTED_MODEL" ]] && [[ $local_size -lt 8192 ]] && [[ $remote_size -lt 8192 ]]; then
        prompt=$(build_prompt "$filename" "$local_content" "$remote_content")
        if merged=$(ollama_merge "$prompt"); then
            used_llm=true
        fi
    fi

    # Validate LLM output
    if $used_llm && [[ -n "$merged" ]] && [[ ${#merged} -ge 20 ]]; then
        # For topic files with frontmatter, verify frontmatter is preserved
        if [[ "$filename" != "MEMORY.md" ]] && head -1 "$local_file" | grep -q '^---'; then
            if ! echo "$merged" | head -1 | grep -q '^---'; then
                log "LLM dropped frontmatter for $encoded/$filename — falling back"
                used_llm=false
            fi
        fi
    else
        used_llm=false
    fi

    if $used_llm; then
        printf '%s\n' "$merged" > "$local_file"
        ((MERGE_COUNT++))
        log "LLM-merged: $encoded/$filename"
    else
        # Text-based fallback
        fallback_result=$(text_fallback_merge "$filename" "$local_file" "$remote_file")
        if [[ -n "$fallback_result" ]]; then
            printf '%s\n' "$fallback_result" > "$local_file"
            ((FALLBACK_COUNT++))
            log "Text-merged (fallback): $encoded/$filename"
        else
            # Restore backup — something went wrong
            cp "${local_file}.pre-merge" "$local_file"
            log "FAILED merge for $encoded/$filename — kept local"
        fi
    fi
done

# ── Phase 7: Push merged results back to Munin ───────────────────────
if [[ $MERGE_COUNT -gt 0 || $FALLBACK_COUNT -gt 0 || $COPY_COUNT -gt 0 ]]; then
    for encoded in "${!ALL_PROJECTS[@]}"; do
        local_mem="$SYNC_CACHE/projects/$encoded/memory/"
        [[ -d "$local_mem" ]] || continue
        rsync $RSYNC_OPTS \
            "$local_mem" \
            "$REMOTE_USER@$MUNIN_IP:$REMOTE_BASE/projects/$encoded/memory/" 2>/dev/null || true
    done
    log "Pushed merged results to Munin"
fi

# ── Phase 8: Hook output ─────────────────────────────────────────────
TOTAL=$((MERGE_COUNT + FALLBACK_COUNT + COPY_COUNT))
if [[ $TOTAL -gt 0 ]]; then
    summary="Memory merge: ${MERGE_COUNT} LLM-merged, ${FALLBACK_COUNT} text-merged, ${COPY_COUNT} copied from remote."
    log "$summary"
    printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":%s}}\n' \
        "$(echo "$summary" | jq -Rs .)"

    notify-send -t 4000 -i dialog-information "[session] memory merged" \
        "$summary" 2>/dev/null || true
else
    log "No changes needed"
fi

# Clean up staging
rm -rf "$STAGING"

exit 0
