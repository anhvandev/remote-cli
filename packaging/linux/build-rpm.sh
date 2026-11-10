#!/bin/sh
# Builds the `rcli` .rpm for this machine's architecture. Run it on Linux (rpmbuild is required).
#
#   sudo dnf install rpm-build
#   packaging/linux/build-rpm.sh                 # -> packaging/linux/out/rcli-<version>-1.<arch>.rpm
#
# Same payload and the same maintainer scripts as the .deb: the package only places /usr/bin/rcli and its hooks
# call `rcli agent install` / `rcli agent uninstall`, so no install logic is duplicated here.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
VERSION=${RCLI_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)}
ARCH=${RCLI_ARCH:-$(rpm --eval '%{_arch}')}
OUT=${RCLI_OUT:-$ROOT/packaging/linux/out}
TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}
TOP=$(mktemp -d)
trap 'rm -rf "$TOP"' EXIT

echo "building rcli $VERSION for $ARCH"
cargo build --release --manifest-path "$ROOT/Cargo.toml" -p rcli

mkdir -p "$TOP/BUILD" "$TOP/RPMS" "$TOP/SOURCES" "$TOP/SPECS" "$TOP/SRPMS"
spec="$TOP/SPECS/rcli.spec"
{
    cat <<SPEC
Name: rcli
Version: $VERSION
Release: 1
Summary: Remote CLI Agent and Controller
License: AGPL-3.0-only
BuildArch: $ARCH
Requires: systemd
Recommends: tesseract-ocr, xauth
%description
One binary for both sides: the Agent runs as a root systemd service and lets Controllers see and use the screen,
run commands and shells, and manage power. The Device registers itself with the Server Key.
%prep
%build
%install
install -D -m 0755 $TARGET_DIR/release/rcli %{buildroot}/usr/bin/rcli
%files
/usr/bin/rcli
%post
SPEC
    cat "$ROOT/packaging/linux/scripts/postinst"
    echo "%preun"
    cat "$ROOT/packaging/linux/scripts/prerm"
    echo "%postun"
    cat "$ROOT/packaging/linux/scripts/postrm"
} > "$spec"

mkdir -p "$OUT"
rpmbuild --define "_topdir $TOP" --define "_rpmdir $OUT" -bb "$spec" >/dev/null
find "$OUT" -maxdepth 2 -name "rcli-$VERSION-1.*.rpm" -exec sh -c 'echo "wrote $1"' _ {} \;
