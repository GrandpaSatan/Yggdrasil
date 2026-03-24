#!/usr/bin/env bash
# PostToolUse hook: ingest significant tool actions into Mimir memory (v2).
# Called by Claude Code with CLAUDE_TOOL_INPUT and CLAUDE_TOOL_OUTPUT env vars.
# NEVER exits non-zero — hook failures must not block tool execution.

[ -f /tmp/ygg-hooks/env ] && . /tmp/ygg-hooks/env
MIMIR_URL="${MIMIR_URL:-http://localhost:9090}"
workstation=$(hostname)

input="${CLAUDE_TOOL_INPUT:-{}}"
output="${CLAUDE_TOOL_OUTPUT:-}"

# Detect tool type: if input has file_path or path field → Edit/Write, else Bash
has_file=$(echo "$input" | jq -r 'if (.file_path or .path) then "yes" else "no" end' 2>/dev/null || echo "no")

if [ "$has_file" = "yes" ]; then
    source_tool="Edit"
    file_path=$(echo "$input" | jq -r '.file_path // .path // ""' 2>/dev/null || echo "")
    raw_content=$(echo "$input" | jq -r '.new_string // .content // ""' 2>/dev/null || echo "")
    content="${raw_content:0:300}"
else
    source_tool="Bash"
    file_path=""
    command=$(echo "$input" | jq -r '.command // ""' 2>/dev/null || echo "")
    output_snippet="${output:0:200}"
    content="${command:0:200} -> ${output_snippet}"
fi

filename=$(basename "${file_path:-$source_tool}")

# Skip content that is too short to be meaningful (e.g. empty Bash " -> ")
if [ ${#content} -lt 20 ]; then
    printf "\033[0;90m[mem]\033[0m -> skip %s: too short (%s chars)\n" "$filename" "${#content}" >&2
    exit 0
fi

# Call Mimir auto-ingest (synchronous, short timeout — returns fast)
payload=$(jq -n \
    --arg content "$content" \
    --arg source "$source_tool" \
    --arg workstation "$workstation" \
    --arg file_path "$file_path" \
    '{content: $content, source: $source, event_type: "post_tool", workstation: $workstation, file_path: $file_path}')

response=$(curl --silent --max-time 3 \
    -H "Content-Type: application/json" \
    -d "$payload" \
    "${MIMIR_URL}/api/v1/auto-ingest" 2>/dev/null) || true

stored=$(echo "$response" | jq -r '.stored // false' 2>/dev/null || echo "false")

if [ "$stored" = "true" ]; then
    template=$(echo "$response" | jq -r '.matched_template // "unknown"' 2>/dev/null || echo "unknown")
    sim=$(echo "$response" | jq -r '.similarity // 0' 2>/dev/null | awk '{printf "%.2f", $1}' 2>/dev/null || echo "0.00")
    printf "\033[0;32m[mem]\033[0m -> stored %s: %s (%.2s)\n" "$filename" "$template" "$sim" >&2
    notify-send -t 2000 -i dialog-information "[mem] stored" "$filename: $template" 2>/dev/null || true
else
    reason=$(echo "$response" | jq -r '.skipped_reason // "no_response"' 2>/dev/null || echo "no_response")
    printf "\033[0;90m[mem]\033[0m -> skip %s: %s\n" "$filename" "$reason" >&2
fi

exit 0
