#!/usr/bin/env bash
# SessionStart hook: initialize env, timing log, and DBUS for the current session.
# Called by Claude Code at the start of each session.
rm -rf /tmp/ygg-hooks
mkdir -p /tmp/ygg-hooks
echo "# Hook timing log - $(date -Iseconds)" > /tmp/ygg-hooks/recall-timing.log
cat > /tmp/ygg-hooks/env <<EOF
MIMIR_URL=http://10.0.65.9:9090
DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-unix:path=/run/user/$(id -u)/bus}
DISPLAY=${DISPLAY:-:1}
EOF
exit 0
