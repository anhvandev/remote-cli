# 31: macOS: launchd install, lock screen, login window (view-only)

**What to build:** Install a root LaunchDaemon plus a LaunchAgent limited to the LoginWindow and Aqua sessions. The installer guides the one-time Screen Recording and Accessibility grants. At the lock screen, capture and input work. At the login window, capture works and input is reported as unavailable, per ADR 0001 and the lockscreen probe.

**Blocked by:** 30

**Status:** wontfix

- [ ] Installer enrolls the Device and installs both launchd jobs; guides the permission grants
- [ ] Contract suite passes at the lock screen (capture + key)
- [ ] At the login window: Observations work; input Actions fail with a clear "input unavailable at macOS login window" error
- [ ] Device detail (CLI and Dashboard) shows when a Mac is at the login window and needs a human login or auto-login

## Comments

- 2026-11-09: Closed. Superseded by `.scratch/easy-setup/issues/24-macos-launchd-app-installer.md` (new install flow: Server Key, Device ID + Device Password, ADR 0003/0004).
