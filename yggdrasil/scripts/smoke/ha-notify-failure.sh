#!/usr/bin/env bash
# Sprint 066 — shared HA mobile-notification helper.
#
# Used by the daily e2e cron wrapper AND the sync_docs_tool sprint_end hook
# whenever an E2E run fails. Always exits 0 so callers' systemd / hook units
# don't enter a failed state — the HA push notification IS the user-facing signal.
#
# Usage:
#   ha-notify-failure.sh <log_file> <title>
#
# Required env (loaded by the caller from /opt/yggdrasil/.env):
#   HA_URL    e.g. http://10.0.65.14:8123
#   HA_TOKEN  long-lived access token
#
# Optional env:
#   YGG_E2E_NOTIFY_TARGET  HA notify service name (default: mobile_app_pixel_10_pro_fold)
#   YGG_E2E_LOG_TAIL       lines of log to include in the notification (default: 40)

set -uo pipefail

LOG_FILE="${1:-}"
TITLE="${2:-Yggdrasil E2E FAILED}"
TAIL_LINES="${YGG_E2E_LOG_TAIL:-40}"
HA_URL="${HA_URL:-}"
HA_TOKEN="${HA_TOKEN:-}"
NOTIFY_TARGET="${YGG_E2E_NOTIFY_TARGET:-mobile_app_pixel_10_pro_fold}"

if [[ -z "$HA_URL" || -z "$HA_TOKEN" ]]; then
    echo "ERROR: HA_URL or HA_TOKEN unset; cannot notify" >&2
    exit 0
fi

if [[ -z "$LOG_FILE" || ! -f "$LOG_FILE" ]]; then
    TAIL="(log file unavailable: ${LOG_FILE:-<unset>})"
else
    TAIL=$(tail -n "$TAIL_LINES" "$LOG_FILE" 2>/dev/null || echo "(log read failed)")
fi

# Compose JSON payload safely (jq if available, else sed-escape fallback).
if command -v jq >/dev/null 2>&1; then
    PAYLOAD=$(jq -nc \
        --arg title "$TITLE" \
        --arg msg "$TAIL" \
        '{title: $title, message: $msg, data: {tag: "ygg-e2e", channel: "yggdrasil-alerts"}}')
else
    ESCAPED=$(printf '%s' "$TAIL" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/\\t/g; s/$/\\n/' | tr -d '\n' | sed 's/\\n$//')
    PAYLOAD="{\"title\":\"${TITLE}\",\"message\":\"${ESCAPED}\",\"data\":{\"tag\":\"ygg-e2e\",\"channel\":\"yggdrasil-alerts\"}}"
fi

NOTIFY_URL="${HA_URL%/}/api/services/notify/${NOTIFY_TARGET}"
HTTP_CODE=$(curl -sS -o /tmp/ha-notify-resp.txt -w '%{http_code}' \
    -X POST "$NOTIFY_URL" \
    -H "Authorization: Bearer $HA_TOKEN" \
    -H 'Content-Type: application/json' \
    --max-time 10 \
    -d "$PAYLOAD" 2>&1) || HTTP_CODE="000"

echo "[$(date -Is)] HA notify POST $NOTIFY_URL → HTTP $HTTP_CODE"
if [[ "$HTTP_CODE" != "200" ]]; then
    echo "WARN: HA notify did not return 200; response: $(cat /tmp/ha-notify-resp.txt 2>/dev/null | head -c 200)" >&2
fi

exit 0
