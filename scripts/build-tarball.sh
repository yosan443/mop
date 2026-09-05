#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-1.0.0}"
ARCH="${ARCH:-$(uname -m)}"
TARGET_TAR_DIR="${ROOT_DIR}/target/tarball"
STAGE_DIR="${TARGET_TAR_DIR}/mop-${VERSION}-linux-${ARCH}"

echo "=== Building standalone tarball for mop v${VERSION} (${ARCH}) ==="

# 1. Build web frontend
if [ "${SKIP_WEB:-false}" != "true" ]; then
    echo "--> Building web frontend..."
    cd "${ROOT_DIR}/web"
    if command -v pnpm >/dev/null 2>&1; then
        pnpm install --frozen-lockfile || pnpm install
        pnpm build
    else
        echo "pnpm is required to build web frontend" >&2
        exit 1
    fi
fi

# 2. Build Rust binaries in release mode
RELEASE_BIN="${RELEASE_BIN:-${ROOT_DIR}/target/release}"
if [ "${SKIP_BUILD:-false}" != "true" ]; then
    echo "--> Building Rust binaries (release)..."
    cd "${ROOT_DIR}"
    cargo build --release \
        -p mop-cli \
        -p mop-plugin-manga \
        -p mop-plugin-video \
        -p mop-plugin-hello
fi

mkdir -p "${TARGET_TAR_DIR}"
rm -rf "${STAGE_DIR}" "${TARGET_TAR_DIR}/mop-${VERSION}-linux-${ARCH}.tar.gz"
mkdir -p "${STAGE_DIR}/bin" \
    "${STAGE_DIR}/plugins/mop.manga/${VERSION}/ui" \
    "${STAGE_DIR}/plugins/mop.video/${VERSION}/ui" \
    "${STAGE_DIR}/plugins/mop.hello/${VERSION}/ui" \
    "${STAGE_DIR}/deploy"

# Copy main binary
cp "${RELEASE_BIN}/mop" "${STAGE_DIR}/bin/mop"
chmod 0755 "${STAGE_DIR}/bin/mop"

# Copy plugins
cp "${RELEASE_BIN}/mop-plugin-manga" "${STAGE_DIR}/plugins/mop.manga/${VERSION}/mop-plugin-manga"
cp "${ROOT_DIR}/plugins/manga/plugin.toml" "${STAGE_DIR}/plugins/mop.manga/${VERSION}/plugin.toml"
cp "${ROOT_DIR}/plugins/manga/ui/index.js" "${STAGE_DIR}/plugins/mop.manga/${VERSION}/ui/index.js"

cp "${RELEASE_BIN}/mop-plugin-video" "${STAGE_DIR}/plugins/mop.video/${VERSION}/mop-plugin-video"
cp "${ROOT_DIR}/plugins/video/plugin.toml" "${STAGE_DIR}/plugins/mop.video/${VERSION}/plugin.toml"
cp "${ROOT_DIR}/plugins/video/ui/index.js" "${STAGE_DIR}/plugins/mop.video/${VERSION}/ui/index.js"

cp "${RELEASE_BIN}/mop-plugin-hello" "${STAGE_DIR}/plugins/mop.hello/${VERSION}/mop-plugin-hello"
cp "${ROOT_DIR}/plugins/hello/plugin.toml" "${STAGE_DIR}/plugins/mop.hello/${VERSION}/plugin.toml"
cp "${ROOT_DIR}/plugins/hello/ui/index.js" "${STAGE_DIR}/plugins/mop.hello/${VERSION}/ui/index.js"

# Copy deploy files
cp "${ROOT_DIR}/deploy/mop.service" "${STAGE_DIR}/deploy/mop.service"
cp "${ROOT_DIR}/deploy/50-mop.rules" "${STAGE_DIR}/deploy/50-mop.rules"

# Example config
cat << 'EOF' > "${STAGE_DIR}/config.toml.example"
[server]
bind = "127.0.0.1:8787"

[database]
path = "/var/lib/mop/mop.db"

[auth]
registration = "first_user"
min_password_len = 10
session_hours = 12

[resources.systemd]
units = ["caddy.service", "nginx.service"]
allow_actions = ["start", "stop", "restart"]

[resources.docker]
containers = ["komga"]
label_selector = "mop.managed=true"
allow_actions = ["start", "stop", "restart"]
EOF

# install.sh
cat << 'EOF' > "${STAGE_DIR}/install.sh"
#!/usr/bin/env bash
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "Error: install.sh must be run as root (sudo ./install.sh)" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DEST="/usr/local/bin/mop"

echo "=== Installing mop ==="

# 1. Groups & Users
echo "--> Creating system users and groups..."
if ! getent group mop >/dev/null; then
    addgroup --system mop
fi
if ! getent group mop-ipc >/dev/null; then
    addgroup --system mop-ipc
fi

if ! getent passwd mop >/dev/null; then
    adduser --system --ingroup mop --home /var/lib/mop --no-create-home --gecos "mop daemon" mop
fi

adduser mop mop-ipc || true
if getent group systemd-journal >/dev/null; then
    adduser mop systemd-journal || true
fi

# Dedicated plugin users
if ! getent passwd mop-plugin-manga >/dev/null; then
    adduser --system --ingroup mop-ipc --home /nonexistent --no-create-home --gecos "mop manga plugin" mop-plugin-manga
fi
if ! getent passwd mop-plugin-video >/dev/null; then
    adduser --system --ingroup mop-ipc --home /nonexistent --no-create-home --gecos "mop video plugin" mop-plugin-video
fi

# 2. Directories
echo "--> Setting up state and configuration directories..."
mkdir -p /etc/mop /var/lib/mop/plugins /var/lib/mop/backups /var/log/mop
chown -R mop:mop /var/lib/mop /var/log/mop
chmod 0750 /var/lib/mop /var/log/mop

# 3. Binary
echo "--> Installing binary to ${BIN_DEST}..."
cp "${SCRIPT_DIR}/bin/mop" "${BIN_DEST}"
chmod 0755 "${BIN_DEST}"

# 4. Plugins
echo "--> Installing first-party plugins to /var/lib/mop/plugins..."
cp -r "${SCRIPT_DIR}/plugins/"* /var/lib/mop/plugins/
chown -R root:root /var/lib/mop/plugins
chmod -R 0755 /var/lib/mop/plugins

# 5. Configuration
if [ ! -f /etc/mop/config.toml ]; then
    echo "--> Installing initial configuration to /etc/mop/config.toml..."
    cp "${SCRIPT_DIR}/config.toml.example" /etc/mop/config.toml
    chown root:mop /etc/mop/config.toml
    chmod 0640 /etc/mop/config.toml
else
    echo "--> Existing /etc/mop/config.toml preserved."
fi

# 6. Polkit rules
if [ -d /etc/polkit-1/rules.d ]; then
    echo "--> Generating polkit rules..."
    "${BIN_DEST}" --config /etc/mop/config.toml polkit-rules --output /etc/polkit-1/rules.d/50-mop.rules || true
    chmod 0644 /etc/polkit-1/rules.d/50-mop.rules 2>/dev/null || true
fi

# 7. systemd service
if [ -d /etc/systemd/system ]; then
    echo "--> Installing systemd service..."
    # Adjust ExecStart path if installing to /usr/local/bin
    sed "s|ExecStart=/usr/bin/mop|ExecStart=${BIN_DEST}|g" "${SCRIPT_DIR}/deploy/mop.service" > /etc/systemd/system/mop.service
    chmod 0644 /etc/systemd/system/mop.service
    systemctl daemon-reload
    systemctl enable mop.service
    echo "--> mop.service enabled. Start it with: sudo systemctl start mop.service"
fi

echo "=== mop installation complete! ==="
EOF
chmod 0755 "${STAGE_DIR}/install.sh"

# uninstall.sh
cat << 'EOF' > "${STAGE_DIR}/uninstall.sh"
#!/usr/bin/env bash
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "Error: uninstall.sh must be run as root (sudo ./uninstall.sh)" >&2
    exit 1
fi

echo "=== Uninstalling mop ==="

if [ -d /run/systemd/system ]; then
    systemctl stop mop.service 2>/dev/null || true
    systemctl disable mop.service 2>/dev/null || true
fi

rm -f /etc/systemd/system/mop.service
if [ -d /run/systemd/system ]; then
    systemctl daemon-reload 2>/dev/null || true
fi

rm -f /usr/local/bin/mop
rm -f /etc/polkit-1/rules.d/50-mop.rules
rm -rf /var/lib/mop/plugins

echo "Removed binaries, plugins, polkit rules, and systemd service."
echo "Note: Database (/var/lib/mop/mop.db) and configuration (/etc/mop) were preserved."
echo "To remove all data, run: sudo rm -rf /var/lib/mop /etc/mop /var/log/mop"
echo "=== Uninstallation complete ==="
EOF
chmod 0755 "${STAGE_DIR}/uninstall.sh"

# Pack tarball
echo "--> Creating tarball archive..."
TAR_NAME="mop-${VERSION}-linux-${ARCH}.tar.gz"
tar -czf "${TARGET_TAR_DIR}/${TAR_NAME}" -C "${TARGET_TAR_DIR}" "mop-${VERSION}-linux-${ARCH}"

echo "✓ Built standalone tarball: ${TARGET_TAR_DIR}/${TAR_NAME}"
ls -lh "${TARGET_TAR_DIR}/${TAR_NAME}"
