#!/bin/sh
# PROTOTYPE: removes everything install.sh added. Results in /tmp/lockscreen-probe are kept.
sudo rm -f /Library/LaunchAgents/dev.remotecli.lockscreen-probe.plist /usr/local/libexec/lockscreen-probe
launchctl bootout "gui/$(id -u)/dev.remotecli.lockscreen-probe" 2>/dev/null || true
echo "Removed."
