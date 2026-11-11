#!/bin/sh
# Builds rcli.app (with the GUI) and rcli-<version>-macos-<arch>.dmg under target/macos/. Unsigned: the build is
# only ad-hoc signed, so Gatekeeper warns on first open (the Dashboard's Connect page explains how to open it).
# Usage: packaging/macos/build-dmg.sh   (run on a Mac, from anywhere in the repo)
set -eu
: "${VCPKG_ROOT:?set VCPKG_ROOT to a vcpkg with the RustDesk FFmpeg, see Build trên macOS in docs/usage.md}"
cd "$(git rev-parse --show-toplevel)"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m | sed 's/arm64/aarch64/')
cargo build --release -p rcli --features gui
bin="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')/release/rcli"
out=target/macos
app="$out/rcli.app"
rm -rf "$out"
mkdir -p "$app/Contents/MacOS" "$out/dmg"
cp "$bin" "$app/Contents/MacOS/rcli"
sed "s/{{VERSION}}/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
codesign --force --sign - "$app"
cp -R "$app" "$out/dmg/"
ln -s /Applications "$out/dmg/Applications"
hdiutil create -quiet -volname rcli -srcfolder "$out/dmg" -ov -format UDZO "$out/rcli-$version-macos-$arch.dmg"
rm -rf "$out/dmg"
echo "$out/rcli-$version-macos-$arch.dmg"
