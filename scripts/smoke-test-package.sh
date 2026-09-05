#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DEB_DIR="${ROOT_DIR}/target/deb"

echo "=== Starting Smoke Test for mop Debian Packages ==="

# Check packages exist
MOP_DEB="$(ls "${TARGET_DEB_DIR}"/mop_*_*.deb 2>/dev/null | head -n 1 || true)"
MANGA_DEB="$(ls "${TARGET_DEB_DIR}"/mop-plugin-manga_*_*.deb 2>/dev/null | head -n 1 || true)"
VIDEO_DEB="$(ls "${TARGET_DEB_DIR}"/mop-plugin-video_*_*.deb 2>/dev/null | head -n 1 || true)"

if [ -z "${MOP_DEB}" ] || [ -z "${MANGA_DEB}" ] || [ -z "${VIDEO_DEB}" ]; then
    echo "Error: Deb packages not found in ${TARGET_DEB_DIR}. Run scripts/build-deb.sh first." >&2
    exit 1
fi

echo "Packages to test:"
echo "  - mop:               ${MOP_DEB}"
echo "  - mop-plugin-manga:  ${MANGA_DEB}"
echo "  - mop-plugin-video:  ${VIDEO_DEB}"

# ------------------------------------------------------------------------------
# 1. Metadata Verification (dpkg-deb -I)
# ------------------------------------------------------------------------------
echo "--> 1. Verifying metadata (dpkg-deb -I)..."

# mop
MOP_INFO="$(dpkg-deb -I "${MOP_DEB}")"
echo "${MOP_INFO}" | grep -q "^ Package: mop$"
echo "${MOP_INFO}" | grep -q "systemd"
echo "${MOP_INFO}" | grep -q "policykit-1"
echo "${MOP_INFO}" | grep -q "libsqlite3"
echo "${MOP_INFO}" | grep -q "Recommends:.*mop-plugin-manga.*mop-plugin-video"
echo "  ✓ mop control metadata verified"

# mop-plugin-manga
MANGA_INFO="$(dpkg-deb -I "${MANGA_DEB}")"
echo "${MANGA_INFO}" | grep -q "^ Package: mop-plugin-manga$"
echo "${MANGA_INFO}" | grep -q "Depends:.*mop (="
echo "${MANGA_INFO}" | grep -q "libarchive"
echo "${MANGA_INFO}" | grep -q "libvips"
echo "  ✓ mop-plugin-manga control metadata verified"

# mop-plugin-video
VIDEO_INFO="$(dpkg-deb -I "${VIDEO_DEB}")"
echo "${VIDEO_INFO}" | grep -q "^ Package: mop-plugin-video$"
echo "${VIDEO_INFO}" | grep -q "Depends:.*mop (="
echo "${VIDEO_INFO}" | grep -q "ffmpeg"
echo "  ✓ mop-plugin-video control metadata verified"

# ------------------------------------------------------------------------------
# 2. Archive File Structure & Permissions (dpkg-deb -c)
# ------------------------------------------------------------------------------
echo "--> 2. Verifying file structure and permissions (dpkg-deb -c)..."

# mop binary executable
MOP_CONTENTS="$(dpkg-deb -c "${MOP_DEB}")"
echo "${MOP_CONTENTS}" | grep -E "^-rwxr-xr-x .* \./usr/bin/mop$"
echo "${MOP_CONTENTS}" | grep -E "^\-rw-r--r-- .* \./lib/systemd/system/mop\.service$"
echo "${MOP_CONTENTS}" | grep -E "^\-rw-r--r-- .* \./usr/share/polkit-1/rules\.d/49-mop\.rules$"
echo "${MOP_CONTENTS}" | grep -E "^\-rw-r--r-- .* \./etc/mop/config\.toml\.example$"
echo "  ✓ mop file contents and permissions verified"

# manga plugin executable and assets
MANGA_CONTENTS="$(dpkg-deb -c "${MANGA_DEB}")"
echo "${MANGA_CONTENTS}" | grep -E "^-rwxr-xr-x .* \./var/lib/mop/plugins/mop\.manga/.+/mop-plugin-manga$"
echo "${MANGA_CONTENTS}" | grep -E "^-rw-r--r-- .* \./var/lib/mop/plugins/mop\.manga/.+/plugin\.toml$"
echo "${MANGA_CONTENTS}" | grep -E "^-rw-r--r-- .* \./var/lib/mop/plugins/mop\.manga/.+/ui/index\.js$"
echo "  ✓ mop-plugin-manga file contents and permissions verified"

# video plugin executable and assets
VIDEO_CONTENTS="$(dpkg-deb -c "${VIDEO_DEB}")"
echo "${VIDEO_CONTENTS}" | grep -E "^-rwxr-xr-x .* \./var/lib/mop/plugins/mop\.video/.+/mop-plugin-video$"
echo "${VIDEO_CONTENTS}" | grep -E "^-rw-r--r-- .* \./var/lib/mop/plugins/mop\.video/.+/plugin\.toml$"
echo "${VIDEO_CONTENTS}" | grep -E "^-rw-r--r-- .* \./var/lib/mop/plugins/mop\.video/.+/ui/index\.js$"
echo "  ✓ mop-plugin-video file contents and permissions verified"

# ------------------------------------------------------------------------------
# 3. Execution in Extracted Root
# ------------------------------------------------------------------------------
echo "--> 3. Testing runtime in extracted root..."

TMP_DIR="$(mktemp -d)"
cleanup() {
    if [ -n "${SERVER_PID:-}" ]; then
        echo "--> Stopping test server (PID ${SERVER_PID})..."
        kill "${SERVER_PID}" 2>/dev/null || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
    rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

# Extract all 3 packages
dpkg-deb -x "${MOP_DEB}" "${TMP_DIR}"
dpkg-deb -x "${MANGA_DEB}" "${TMP_DIR}"
dpkg-deb -x "${VIDEO_DEB}" "${TMP_DIR}"

MOP_BIN="${TMP_DIR}/usr/bin/mop"

# 3.1 Verify CLI commands
echo "  Testing 'mop --version'..."
VERSION_OUTPUT="$("${MOP_BIN}" --version)"
echo "    Output: ${VERSION_OUTPUT}"
echo "${VERSION_OUTPUT}" | grep -q "mop"

echo "  Testing 'mop --help'..."
HELP_OUTPUT="$("${MOP_BIN}" --help)"
echo "${HELP_OUTPUT}" | grep -q "Usage:"
echo "${HELP_OUTPUT}" | grep -q "serve"
echo "${HELP_OUTPUT}" | grep -q "backup"
echo "${HELP_OUTPUT}" | grep -q "restore"
echo "${HELP_OUTPUT}" | grep -q "polkit-rules"

# 3.2 Verify polkit-rules generation
TEST_CONFIG="${TMP_DIR}/test-config.toml"
TEST_DB="${TMP_DIR}/test-mop.db"
TEST_PORT="18787"

cat << EOF > "${TEST_CONFIG}"
[server]
bind = "127.0.0.1:${TEST_PORT}"

[database]
path = "${TEST_DB}"

[plugins]
dir = "${TMP_DIR}/var/lib/mop/plugins"

[backup]
dir = "${TMP_DIR}/backups"

[resources.systemd]
units = ["caddy.service"]
allow_actions = ["restart"]

[resources.docker]
containers = ["komga"]
label_selector = "mop.managed=true"
allow_actions = ["restart"]
EOF

echo "  Testing 'mop polkit-rules'..."
POLKIT_OUTPUT="$("${MOP_BIN}" --config "${TEST_CONFIG}" polkit-rules)"
echo "${POLKIT_OUTPUT}" | grep -q "polkit.addRule"
echo "${POLKIT_OUTPUT}" | grep -q "caddy.service"
echo "  ✓ polkit rules generated successfully"

# 3.3 Start daemon and verify endpoints
echo "  Starting 'mop serve' on port ${TEST_PORT}..."
"${MOP_BIN}" serve --config "${TEST_CONFIG}" > "${TMP_DIR}/server.log" 2>&1 &
SERVER_PID=$!

echo "    Waiting for server to become healthy..."
MAX_TRIES=30
TRIES=0
HEALTHY=false

while [ $TRIES -lt $MAX_TRIES ]; do
    if curl -s -f "http://127.0.0.1:${TEST_PORT}/health" >/dev/null 2>&1; then
        HEALTHY=true
        break
    fi
    sleep 0.5
    TRIES=$((TRIES + 1))
done

if [ "${HEALTHY}" != "true" ]; then
    echo "Error: Server failed to start within timeout. Logs:" >&2
    cat "${TMP_DIR}/server.log" >&2
    exit 1
fi

echo "  ✓ /health endpoint responded OK"
HEALTH_BODY="$(curl -s "http://127.0.0.1:${TEST_PORT}/health")"
echo "    Response: ${HEALTH_BODY}"
echo "${HEALTH_BODY}" | grep -q '"status":"ok"'

# 3.4 Register initial admin user and verify authentication
echo "  Registering initial admin user..."
COOKIE_JAR="${TMP_DIR}/cookies.txt"
curl -s -f -c "${COOKIE_JAR}" \
    -H "Content-Type: application/json" \
    -H "Origin: http://127.0.0.1:${TEST_PORT}" \
    -d '{"username":"admin","password":"Password12345!"}' \
    "http://127.0.0.1:${TEST_PORT}/api/v1/auth/register" >/dev/null
echo "  ✓ Initial admin registered and authenticated"

# 3.5 Verify plugin detection
echo "  Verifying installed plugins detected by supervisor..."
PLUGINS_RESP="$(curl -s -f -b "${COOKIE_JAR}" "http://127.0.0.1:${TEST_PORT}/api/v1/plugins")"
echo "${PLUGINS_RESP}" | grep -q "mop.manga"
echo "${PLUGINS_RESP}" | grep -q "mop.video"
echo "  ✓ mop.manga and mop.video detected in plugin list API"

# 3.6 Verify plugin asset serving (manga and video UI)
echo "  Verifying plugin assets served from packages..."
MANGA_UI="$(curl -s -f -b "${COOKIE_JAR}" "http://127.0.0.1:${TEST_PORT}/api/v1/plugins/mop.manga/ui/index.js")"
if [ -z "${MANGA_UI}" ]; then
    echo "Error: Failed to fetch mop.manga ui/index.js" >&2
    exit 1
fi
echo "  ✓ mop.manga UI asset successfully served (${#MANGA_UI} bytes)"

VIDEO_UI="$(curl -s -f -b "${COOKIE_JAR}" "http://127.0.0.1:${TEST_PORT}/api/v1/plugins/mop.video/ui/index.js")"
if [ -z "${VIDEO_UI}" ]; then
    echo "Error: Failed to fetch mop.video ui/index.js" >&2
    exit 1
fi
echo "  ✓ mop.video UI asset successfully served (${#VIDEO_UI} bytes)"

# 3.7 Verify embedded SPA index.html is served
INDEX_HTML="$(curl -s -f "http://127.0.0.1:${TEST_PORT}/")"
echo "${INDEX_HTML}" | grep -q "<title>mop</title>" || echo "${INDEX_HTML}" | grep -q "mop"
echo "  ✓ Web frontend embedded SPA index.html served"

echo "=== Smoke Test PASSED successfully! All package components operational. ==="
