#!/bin/sh
# Builds the `rcli` .deb for this machine's architecture. Run it on Linux (dpkg-deb is required).
#
#   sudo apt install dpkg-dev
#   packaging/linux/build-deb.sh                 # -> packaging/linux/out/rcli_<version>_<arch>.deb
#
# The package installs only /usr/bin/rcli. Its postinst runs the same `rcli install` path as the curl
# one-liner, so the service, Device Settings, secret store and registration logic live in exactly one place.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
VERSION=${RCLI_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)}
ARCH=${RCLI_ARCH:-$(dpkg --print-architecture)}
OUT=${RCLI_OUT:-$ROOT/packaging/linux/out}
TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

echo "building rcli $VERSION for $ARCH"
# The GUI window answers the root and accept prompts; it loads the display libraries only when it opens.
# hwcodec: Live View on the GPU through VA-API (libva is loaded at run time, openh264 without it); needs VCPKG_ROOT
# with RustDesk's FFmpeg, see docs/usage.md.
cargo build --release --manifest-path "$ROOT/Cargo.toml" -p rcli --features gui,hwcodec

mkdir -p "$STAGE/usr/bin" "$STAGE/DEBIAN"
# Depends: the libraries the binary links (`readelf -d`): dbus and PipeWire, plus libdrm, libX11 and (x86_64) zlib
# from hwcodec's FFmpeg. rpmbuild finds these by itself.
install -m 0755 "$TARGET_DIR/release/rcli" "$STAGE/usr/bin/rcli"
for hook in postinst prerm postrm; do
    install -m 0755 "$ROOT/packaging/linux/scripts/$hook" "$STAGE/DEBIAN/$hook"
done
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: rcli
Version: $VERSION
Architecture: $ARCH
Maintainer: Remote CLI <dev@remotecli.invalid>
Section: admin
Priority: optional
Depends: systemd, libdbus-1-3, libpipewire-0.3-0 | libpipewire-0.3-0t64, libdrm2, libx11-6, zlib1g, libstdc++6
Recommends: tesseract-ocr, xauth, libva-drm2, libva-x11-2, va-driver-all
Description: remote CLI Agent and Controller
 One binary for both sides: the Agent runs as a root systemd service and lets
 Controllers see and use the screen, run commands and shells, and manage power;
 the CLI is also the Controller. The Device registers itself with the Server Key.
CONTROL

mkdir -p "$OUT"
deb="$OUT/rcli_${VERSION}_${ARCH}.deb"
dpkg-deb --build --root-owner-group "$STAGE" "$deb" >/dev/null
echo "wrote $deb"
