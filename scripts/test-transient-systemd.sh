#!/bin/bash
set -euo pipefail

# test-transient-systemd.sh — Layer 2 Verification Script for mop systemd transient units & polkit
# Usage: sudo bash scripts/test-transient-systemd.sh

echo "=== [1/6] Environment Check ==="
if [ "$(id -u)" -ne 0 ]; then
    echo "Error: This script must be run as root (or via sudo) on a systemd host." >&2
    exit 1
fi

if [ ! -d /run/systemd/system ]; then
    echo "Error: systemd is not running as PID 1 on this system." >&2
    exit 1
fi

echo "Systemd detected: $(systemctl --version | head -n 1)"

echo "=== [2/6] User & Group Setup (mop, mop-ipc) ==="
if ! getent group mop >/dev/null; then
    addgroup --system mop
fi
if ! getent group mop-ipc >/dev/null; then
    addgroup --system mop-ipc
fi
if ! getent passwd mop >/dev/null; then
    adduser --system --ingroup mop --home /var/lib/mop --no-create-home mop
fi
adduser mop mop-ipc || true

echo "=== [3/6] Polkit Rules Generation & Verification ==="
POLKIT_RULES_DIR="/etc/polkit-1/rules.d"
mkdir -p "$POLKIT_RULES_DIR"
RULE_FILE="${POLKIT_RULES_DIR}/50-mop.rules"

cargo run -p mop-cli -- polkit-rules --output "$RULE_FILE"
chmod 0644 "$RULE_FILE"
echo "Generated polkit rule at $RULE_FILE:"
cat "$RULE_FILE"

# Restart polkit to load new rules if running
if systemctl is-active polkit >/dev/null 2>&1; then
    systemctl restart polkit
    echo "Restarted polkit service."
fi

echo "=== [4/6] Runtime Directory & Permission Setup ==="
RUN_DIR="/run/mop"
PLUGINS_RUN_DIR="${RUN_DIR}/plugins"
mkdir -p "$PLUGINS_RUN_DIR"
chown mop:mop "$RUN_DIR"
chmod 0755 "$RUN_DIR"

chown mop:mop-ipc "$PLUGINS_RUN_DIR"
chmod 2770 "$PLUGINS_RUN_DIR"

# Create dummy host.sock with 0660 mop:mop-ipc
HOST_SOCK="${RUN_DIR}/host.sock"
rm -f "$HOST_SOCK"
python3 -c "import socket; s = socket.socket(socket.AF_UNIX); s.bind('$HOST_SOCK')"
chown mop:mop-ipc "$HOST_SOCK"
chmod 0660 "$HOST_SOCK"

echo "Directory permissions:"
ls -ld "$RUN_DIR" "$PLUGINS_RUN_DIR" "$HOST_SOCK"

echo "=== [5/6] Testing Transient Unit with DynamicUser & SupplementaryGroups ==="
TEST_UNIT="mop-plugin-mop.test.service"
systemctl stop "$TEST_UNIT" 2>/dev/null || true

# Launch transient unit as user mop-plugin-test with DynamicUser=yes and SupplementaryGroups=mop-ipc
systemd-run \
    --unit="$TEST_UNIT" \
    --description="mop test transient unit" \
    --property="DynamicUser=yes" \
    --property="User=mop-plugin-mop.test" \
    --property="SupplementaryGroups=mop-ipc" \
    --property="WorkingDirectory=/tmp" \
    sleep 30

echo "Transient unit started: $TEST_UNIT"
sleep 1

# Check unit status and MainPID
MAIN_PID=$(systemctl show "$TEST_UNIT" --property=MainPID --value)
ACTIVE_STATE=$(systemctl show "$TEST_UNIT" --property=ActiveState --value)

echo "ActiveState: $ACTIVE_STATE"
echo "MainPID:     $MAIN_PID"

if [ -z "$MAIN_PID" ] || [ "$MAIN_PID" -le 0 ]; then
    echo "FAIL: Could not retrieve valid MainPID for $TEST_UNIT" >&2
    exit 1
fi

echo "=== [6/6] Testing Plugin Socket Access & Group Inheritance ==="
# Test that process running inside transient unit can write to $HOST_SOCK
systemd-run \
    --unit="mop-plugin-ipc-check.service" \
    --property="DynamicUser=yes" \
    --property="SupplementaryGroups=mop-ipc" \
    --wait \
    python3 -c "import socket; s = socket.socket(socket.AF_UNIX); s.connect('$HOST_SOCK'); print('SUCCESS: Connected to host.sock from DynamicUser in mop-ipc group')"

# Clean up test units
systemctl stop "$TEST_UNIT" 2>/dev/null || true
systemctl stop "mop-plugin-ipc-check.service" 2>/dev/null || true
rm -f "$HOST_SOCK"

echo "=== ALL TRANSIENT SYSTEMD & POLKIT TESTS PASSED ==="
