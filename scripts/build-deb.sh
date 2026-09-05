#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-0.1.0}"
ARCH="${ARCH:-$(dpkg --print-architecture 2>/dev/null || echo amd64)}"
TARGET_DEB_DIR="${ROOT_DIR}/target/deb"

echo "=== Building debian packages for mop v${VERSION} (${ARCH}) ==="

# 1. Build web frontend
echo "--> Building web frontend..."
cd "${ROOT_DIR}/web"
if command -v pnpm >/dev/null 2>&1; then
    pnpm install --frozen-lockfile || pnpm install
    pnpm build
else
    echo "pnpm is required to build web frontend" >&2
    exit 1
fi

# 2. Build Rust binaries in release mode
echo "--> Building Rust binaries (release)..."
cd "${ROOT_DIR}"
cargo build --release \
    -p mop-cli \
    -p mop-plugin-manga \
    -p mop-plugin-video \
    -p mop-plugin-hello

RELEASE_BIN="${ROOT_DIR}/target/release"

# Prepare target and staging directories
rm -rf "${TARGET_DEB_DIR}"
mkdir -p "${TARGET_DEB_DIR}"

STAGE_BASE="$(mktemp -d /tmp/mop-deb-stage.XXXXXX)"
trap 'rm -rf "${STAGE_BASE}"' EXIT

# ------------------------------------------------------------------------------
# 3. Package: mop
# ------------------------------------------------------------------------------
echo "--> Staging mop package..."
MOP_STAGE="${STAGE_BASE}/stage-mop"
rm -rf "${MOP_STAGE}"
mkdir -p \
    "${MOP_STAGE}/usr/bin" \
    "${MOP_STAGE}/lib/systemd/system" \
    "${MOP_STAGE}/usr/share/polkit-1/rules.d" \
    "${MOP_STAGE}/etc/mop" \
    "${MOP_STAGE}/var/lib/mop/plugins" \
    "${MOP_STAGE}/var/lib/mop/backups" \
    "${MOP_STAGE}/var/log/mop" \
    "${MOP_STAGE}/DEBIAN"

cp "${RELEASE_BIN}/mop" "${MOP_STAGE}/usr/bin/mop"
chmod 0755 "${MOP_STAGE}/usr/bin/mop"

cp "${ROOT_DIR}/deploy/mop.service" "${MOP_STAGE}/lib/systemd/system/mop.service"
chmod 0644 "${MOP_STAGE}/lib/systemd/system/mop.service"

cp "${ROOT_DIR}/deploy/50-mop.rules" "${MOP_STAGE}/usr/share/polkit-1/rules.d/49-mop.rules"
chmod 0644 "${MOP_STAGE}/usr/share/polkit-1/rules.d/49-mop.rules"

# Example config
cat << 'EOF' > "${MOP_STAGE}/etc/mop/config.toml.example"
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
chmod 0644 "${MOP_STAGE}/etc/mop/config.toml.example"

# mop control
cat << EOF > "${MOP_STAGE}/DEBIAN/control"
Package: mop
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: mop contributors <yosan443@users.noreply.github.com>
Depends: systemd, policykit-1 | polkitd, libc6, libsqlite3-0, libsystemd0, adduser
Recommends: mop-plugin-manga, mop-plugin-video
Description: Master-of-Process daemon and management web console
 mop is a lightweight, secure process supervisor and Docker/Systemd
 management console with embedded Vue 3 SPA and PWA support.
EOF

# mop postinst
cat << 'EOF' > "${MOP_STAGE}/DEBIAN/postinst"
#!/bin/sh
set -e

case "$1" in
    configure)
        # Create mop group and mop-ipc group if not existing
        if ! getent group mop >/dev/null; then
            addgroup --system mop
        fi
        if ! getent group mop-ipc >/dev/null; then
            addgroup --system mop-ipc
        fi

        # Create mop user
        if ! getent passwd mop >/dev/null; then
            adduser --system \
                --ingroup mop \
                --home /var/lib/mop \
                --no-create-home \
                --gecos "mop daemon" \
                mop
        fi

        # Add mop user to mop-ipc and systemd-journal
        adduser mop mop-ipc || true
        if getent group systemd-journal >/dev/null; then
            adduser mop systemd-journal || true
        fi

        # Set permissions for state directories
        mkdir -p /var/lib/mop/plugins /var/lib/mop/backups /var/log/mop /etc/mop
        chown -R mop:mop /var/lib/mop /var/log/mop
        chmod 0750 /var/lib/mop /var/log/mop

        # Default config if not present
        if [ ! -f /etc/mop/config.toml ] && [ -f /etc/mop/config.toml.example ]; then
            cp /etc/mop/config.toml.example /etc/mop/config.toml
            chown root:mop /etc/mop/config.toml
            chmod 0640 /etc/mop/config.toml
        fi

        # Generate polkit rules if mop binary is available
        if command -v mop >/dev/null 2>&1 && [ -f /etc/mop/config.toml ]; then
            mkdir -p /etc/polkit-1/rules.d
            mop --config /etc/mop/config.toml polkit-rules --output /etc/polkit-1/rules.d/50-mop.rules || true
            chmod 0644 /etc/polkit-1/rules.d/50-mop.rules 2>/dev/null || true
        fi

        # Reload systemd
        if [ -d /run/systemd/system ]; then
            systemctl --system daemon-reload >/dev/null || true
            systemctl enable mop.service >/dev/null || true
        fi
    ;;
esac

exit 0
EOF
chmod 0755 "${MOP_STAGE}/DEBIAN/postinst"

# mop prerm
cat << 'EOF' > "${MOP_STAGE}/DEBIAN/prerm"
#!/bin/sh
set -e

case "$1" in
    remove|deconfigure)
        if [ -d /run/systemd/system ]; then
            systemctl stop mop.service >/dev/null 2>&1 || true
            systemctl disable mop.service >/dev/null 2>&1 || true
        fi
    ;;
esac

exit 0
EOF
chmod 0755 "${MOP_STAGE}/DEBIAN/prerm"

# mop postrm
cat << 'EOF' > "${MOP_STAGE}/DEBIAN/postrm"
#!/bin/sh
set -e

case "$1" in
    remove)
        if [ -d /run/systemd/system ]; then
            systemctl --system daemon-reload >/dev/null 2>&1 || true
        fi
    ;;
    purge)
        rm -rf /var/log/mop
        rm -f /etc/polkit-1/rules.d/50-mop.rules
        if [ -d /run/systemd/system ]; then
            systemctl --system daemon-reload >/dev/null 2>&1 || true
        fi
    ;;
esac

exit 0
EOF
chmod 0755 "${MOP_STAGE}/DEBIAN/postrm"
chmod 0644 "${MOP_STAGE}/DEBIAN/control"
chmod 0755 "${MOP_STAGE}/DEBIAN"

dpkg-deb --root-owner-group --build "${MOP_STAGE}" "${TARGET_DEB_DIR}/mop_${VERSION}_${ARCH}.deb"
echo "✓ Built ${TARGET_DEB_DIR}/mop_${VERSION}_${ARCH}.deb"

# ------------------------------------------------------------------------------
# 4. Package: mop-plugin-manga
# ------------------------------------------------------------------------------
echo "--> Staging mop-plugin-manga package..."
MANGA_STAGE="${STAGE_BASE}/stage-mop-plugin-manga"
MANGA_PLUGIN_DIR="${MANGA_STAGE}/var/lib/mop/plugins/mop.manga/${VERSION}"
rm -rf "${MANGA_STAGE}"
mkdir -p "${MANGA_PLUGIN_DIR}/ui" "${MANGA_STAGE}/DEBIAN"

cp "${RELEASE_BIN}/mop-plugin-manga" "${MANGA_PLUGIN_DIR}/mop-plugin-manga"
chmod 0755 "${MANGA_PLUGIN_DIR}/mop-plugin-manga"
cp "${ROOT_DIR}/plugins/manga/plugin.toml" "${MANGA_PLUGIN_DIR}/plugin.toml"
chmod 0644 "${MANGA_PLUGIN_DIR}/plugin.toml"
cp "${ROOT_DIR}/plugins/manga/ui/index.js" "${MANGA_PLUGIN_DIR}/ui/index.js"
chmod 0644 "${MANGA_PLUGIN_DIR}/ui/index.js"

cat << EOF > "${MANGA_STAGE}/DEBIAN/control"
Package: mop-plugin-manga
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: mop contributors <yosan443@users.noreply.github.com>
Depends: mop (= ${VERSION}), libarchive13t64 | libarchive13, libvips42t64 | libvips42
Description: Manga to WebP CBZ conversion plugin for mop
 Converts archives (ZIP, RAR, 7z, TAR) into optimized WebP CBZ files
 with automatic image inspection and directory monitoring.
EOF

cat << 'EOF' > "${MANGA_STAGE}/DEBIAN/postinst"
#!/bin/sh
set -e

case "$1" in
    configure)
        # Ensure mop-ipc group exists
        if ! getent group mop-ipc >/dev/null; then
            addgroup --system mop-ipc
        fi

        # Dedicated plugin user
        if ! getent passwd mop-plugin-manga >/dev/null; then
            adduser --system \
                --ingroup mop-ipc \
                --home /nonexistent \
                --no-create-home \
                --gecos "mop manga plugin" \
                mop-plugin-manga
        fi

        # Permissions: directory is readable by mop, executable by mop-plugin-manga
        chown -R root:root /var/lib/mop/plugins/mop.manga
        chmod 0755 /var/lib/mop/plugins/mop.manga

        # Restart mop if running to detect newly installed plugin
        if [ -d /run/systemd/system ]; then
            systemctl try-restart mop.service >/dev/null 2>&1 || true
        fi
    ;;
esac

exit 0
EOF
chmod 0755 "${MANGA_STAGE}/DEBIAN/postinst"

cat << 'EOF' > "${MANGA_STAGE}/DEBIAN/postrm"
#!/bin/sh
set -e

case "$1" in
    remove|purge)
        if [ -d /run/systemd/system ]; then
            systemctl try-restart mop.service >/dev/null 2>&1 || true
        fi
    ;;
esac

exit 0
EOF
chmod 0755 "${MANGA_STAGE}/DEBIAN/postrm"
chmod 0644 "${MANGA_STAGE}/DEBIAN/control"
chmod 0755 "${MANGA_STAGE}/DEBIAN"

dpkg-deb --root-owner-group --build "${MANGA_STAGE}" "${TARGET_DEB_DIR}/mop-plugin-manga_${VERSION}_${ARCH}.deb"
echo "✓ Built ${TARGET_DEB_DIR}/mop-plugin-manga_${VERSION}_${ARCH}.deb"

# ------------------------------------------------------------------------------
# 5. Package: mop-plugin-video
# ------------------------------------------------------------------------------
echo "--> Staging mop-plugin-video package..."
VIDEO_STAGE="${STAGE_BASE}/stage-mop-plugin-video"
VIDEO_PLUGIN_DIR="${VIDEO_STAGE}/var/lib/mop/plugins/mop.video/${VERSION}"
rm -rf "${VIDEO_STAGE}"
mkdir -p "${VIDEO_PLUGIN_DIR}/ui" "${VIDEO_STAGE}/DEBIAN"

cp "${RELEASE_BIN}/mop-plugin-video" "${VIDEO_PLUGIN_DIR}/mop-plugin-video"
chmod 0755 "${VIDEO_PLUGIN_DIR}/mop-plugin-video"
cp "${ROOT_DIR}/plugins/video/plugin.toml" "${VIDEO_PLUGIN_DIR}/plugin.toml"
chmod 0644 "${VIDEO_PLUGIN_DIR}/plugin.toml"
cp "${ROOT_DIR}/plugins/video/ui/index.js" "${VIDEO_PLUGIN_DIR}/ui/index.js"
chmod 0644 "${VIDEO_PLUGIN_DIR}/ui/index.js"

cat << EOF > "${VIDEO_STAGE}/DEBIAN/control"
Package: mop-plugin-video
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: mop contributors <yosan443@users.noreply.github.com>
Depends: mop (= ${VERSION}), ffmpeg
Description: Video transcoding plugin for mop
 Transcodes video files and monitors media directories for mop supervisor.
EOF

cat << 'EOF' > "${VIDEO_STAGE}/DEBIAN/postinst"
#!/bin/sh
set -e

case "$1" in
    configure)
        # Ensure mop-ipc group exists
        if ! getent group mop-ipc >/dev/null; then
            addgroup --system mop-ipc
        fi

        # Dedicated plugin user
        if ! getent passwd mop-plugin-video >/dev/null; then
            adduser --system \
                --ingroup mop-ipc \
                --home /nonexistent \
                --no-create-home \
                --gecos "mop video plugin" \
                mop-plugin-video
        fi

        chown -R root:root /var/lib/mop/plugins/mop.video
        chmod 0755 /var/lib/mop/plugins/mop.video

        if [ -d /run/systemd/system ]; then
            systemctl try-restart mop.service >/dev/null 2>&1 || true
        fi
    ;;
esac

exit 0
EOF
chmod 0755 "${VIDEO_STAGE}/DEBIAN/postinst"

cat << 'EOF' > "${VIDEO_STAGE}/DEBIAN/postrm"
#!/bin/sh
set -e

case "$1" in
    remove|purge)
        if [ -d /run/systemd/system ]; then
            systemctl try-restart mop.service >/dev/null 2>&1 || true
        fi
    ;;
esac

exit 0
EOF
chmod 0755 "${VIDEO_STAGE}/DEBIAN/postrm"
chmod 0644 "${VIDEO_STAGE}/DEBIAN/control"
chmod 0755 "${VIDEO_STAGE}/DEBIAN"

dpkg-deb --root-owner-group --build "${VIDEO_STAGE}" "${TARGET_DEB_DIR}/mop-plugin-video_${VERSION}_${ARCH}.deb"
echo "✓ Built ${TARGET_DEB_DIR}/mop-plugin-video_${VERSION}_${ARCH}.deb"

echo "=== Successfully built all 3 debian packages in ${TARGET_DEB_DIR} ==="
ls -lh "${TARGET_DEB_DIR}"/*.deb
