# 24: macOS: `.app`/`.dmg`, launchd, lock screen, login window (view-only)

**What to build:** Replaces `.scratch/remote-cli/issues/31`. Ship `rcli` as an `.app` in a `.dmg`, so TCC grants stick to the bundle. Opening it shows the GUI with Install, and the curl one-liner and `rcli agent install` also work. Install a root LaunchDaemon plus a LaunchAgent limited to the LoginWindow and Aqua sessions. The GUI guides the one-time Screen Recording and Accessibility grants. At the lock screen, capture and input work. At the login window, capture works and input is reported as unavailable (ADR 0001). The Secret Store uses the System Keychain. The build is unsigned, so the guide page covers Gatekeeper. See `.scratch/easy-setup/spec.md`.

**Blocked by:** 23, 14, 17

**Status:** ready-for-agent

- [ ] `.dmg` install, the curl one-liner and `rcli agent install` all install both launchd jobs and register the Device; the GUI guides the permission grants
- [ ] Contract suite passes at the lock screen (capture + key)
- [ ] At the login window: Observations work; input Actions fail with a clear 'input unavailable at macOS login window' error
- [ ] Device detail (CLI and Dashboard) shows when a Mac is at the login window
- [ ] Uninstall removes both jobs
