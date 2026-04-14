#!/usr/bin/env bash
# Sprint 066 — daily E2E cron wrapper (rewritten for pytest tests-e2e).
#
# Drives `pytest tests-e2e -m "not destructive and not slow"` against the live
# fleet, captures the log, and on non-zero exit POSTs a notification to Home
# Assistant via the shared ha-notify-failure.sh helper.
#
# Always exits 0 so the systemd unit doesn't enter a failed state — the HA
# push notification IS the user-facing signal.
#
# Required env (read from /opt/yggdrasil/.env via the systemd unit):
#   HA_URL    e.g. http://10.0.65.14:8123
#   HA_TOKEN  long-lived access token
#
# Optional env:
#   YGG_E2E_TESTS_DIR  override pytest root (default: /opt/yggdrasil/tests-e2e)
#   YGG_E2E_PYTEST     pytest binary (default: <tests-dir>/.venv/bin/pytest)
#   ODIN_URL           for the e2e-hit ping (default: http://127.0.0.1:8080)

set -uo pipefail

TESTS_DIR="${YGG_E2E_TESTS_DIR:-/opt/yggdrasil/tests-e2e}"
PYTEST="${YGG_E2E_PYTEST:-${TESTS_DIR}/.venv/bin/pytest}"
ODIN_URL="${ODIN_URL:-http://127.0.0.1:8080}"
NOTIFY_HELPER="$(dirname "$(readlink -f "$0")")/ha-notify-failure.sh"

LOG_DIR="/var/log/yggdrasil"
mkdir -p "$LOG_DIR" 2>/dev/null || LOG_DIR="/tmp"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
LOG_FILE="$LOG_DIR/e2e-${TS}.log"

echo "[$(date -Is)] starting e2e cron wrapper"
echo "  tests-dir : $TESTS_DIR"
echo "  pytest    : $PYTEST"
echo "  log       : $LOG_FILE"

# Sprint 064 P8 — increment odin_e2e_hits_total so Prometheus can see the
# timer firing. Best-effort: never block the run on this.
curl -sS -o /dev/null --max-time 5 -X POST "${ODIN_URL%/}/api/v1/e2e/hit" || \
    echo "WARN: e2e hit ping failed (non-fatal)"

if [[ ! -d "$TESTS_DIR" ]]; then
    echo "ERROR: tests-e2e dir not found at $TESTS_DIR" | tee -a "$LOG_FILE"
    "$NOTIFY_HELPER" "$LOG_FILE" "Yggdrasil E2E missing tests-e2e dir"
    exit 0
fi

if [[ ! -x "$PYTEST" ]]; then
    echo "ERROR: pytest not executable at $PYTEST" | tee -a "$LOG_FILE"
    echo "Hint: cd $TESTS_DIR && python3 -m venv .venv && \\" | tee -a "$LOG_FILE"
    echo "      .venv/bin/pip install pytest pytest-timeout requests tenacity \\" | tee -a "$LOG_FILE"
    echo "      websockets python-dotenv jsonschema" | tee -a "$LOG_FILE"
    "$NOTIFY_HELPER" "$LOG_FILE" "Yggdrasil E2E missing pytest venv"
    exit 0
fi

# Run pytest. The hook-context env var hard-skips destructive tests even if a
# stale developer env leaked E2E_DESTRUCTIVE=1 onto the system.
START_EPOCH=$(date +%s)
(
    cd "$TESTS_DIR" && \
    E2E_HOOK_CONTEXT=cron \
    E2E_DESTRUCTIVE=0 \
    "$PYTEST" \
        -m "not destructive and not slow" \
        --timeout=30 \
        --maxfail=5 \
        -p no:xdist \
        -p no:cacheprovider \
        --tb=short \
        -q
) >"$LOG_FILE" 2>&1
EXIT=$?
END_EPOCH=$(date +%s)
DURATION=$((END_EPOCH - START_EPOCH))

echo "[$(date -Is)] pytest finished exit=$EXIT duration=${DURATION}s"

if [[ "$EXIT" -eq 0 ]]; then
    echo "e2e PASS — no notification sent"
    exit 0
fi

# Failure path: shell out to the shared notify helper.
"$NOTIFY_HELPER" "$LOG_FILE" "Yggdrasil E2E FAILED (exit=$EXIT, ${DURATION}s)"
exit 0
