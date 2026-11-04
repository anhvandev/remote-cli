#!/bin/sh
# PROTOTYPE: installs the probe as a LoginWindow+Aqua LaunchAgent. Undo with uninstall.sh.
set -e
cd "$(dirname "$0")"
swiftc -O -target "$(uname -m)-apple-macos14.0" probe.swift -o lockscreen-probe
sudo install -d /usr/local/libexec
sudo install -m 755 lockscreen-probe /usr/local/libexec/lockscreen-probe
sudo install -m 644 dev.remotecli.lockscreen-probe.plist /Library/LaunchAgents/
echo "Installed. Log out: the probe runs at the login window for ~5 min (30 rounds x 10s)."
