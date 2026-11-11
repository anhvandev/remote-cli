# 29: Windows: service, lock screen, login screen, UAC, Ctrl+Alt+Del

**What to build:** Install the Agent as a SYSTEM service that launches a helper in the active console session using the winlogon token. The helper switches to the input desktop before capture and input and rebuilds capture on desktop changes, so the lock screen, login screen and UAC prompts can be observed and controlled. Ctrl+Alt+Del is sent with SendSAS. Ship an installer that takes an Enrollment Code.

**Blocked by:** 28

**Status:** wontfix

- [ ] Installer enrolls the Device with an Enrollment Code and installs the service
- [ ] Contract suite passes at the lock screen, login screen and a UAC prompt
- [ ] Ctrl+Alt+Del works (document the software SAS policy requirement)
- [ ] `unlock` works at the lock and login screens
- [ ] Session-state detection reports lock/login screen correctly

## Comments

- 2026-11-09: Closed. Superseded by `.scratch/easy-setup/issues/22-windows-service-installer.md` (new install flow: Server Key, Device ID + Device Password, ADR 0003/0004).
