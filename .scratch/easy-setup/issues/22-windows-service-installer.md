# 22: Windows: SYSTEM service, self-installing exe, lock/login/UAC

**What to build:** Replaces `.scratch/remote-cli/issues/29`. Double-clicking `rcli.exe` opens the GUI with an Install button (Server Address + Key). The PowerShell one-liner (`install.ps1`, finished here) and `rcli agent install` in an elevated shell also work. Install copies the binary, records the Terminal User, registers the Device, and installs a SYSTEM service that launches a helper in the active console session using the winlogon token. The helper switches to the input desktop before capture and input and rebuilds capture on desktop changes, so the lock screen, login screen and UAC prompts can be observed and controlled. Ctrl+Alt+Del is sent with SendSAS. The Secret Store uses DPAPI (machine scope). Uninstall removes everything. The build is unsigned, so the guide page covers SmartScreen. See `.scratch/easy-setup/spec.md`.

**Blocked by:** 21, 14, 17

**Status:** ready-for-agent

- [ ] GUI install, the PowerShell one-liner and `rcli agent install` all install the service and register the Device
- [ ] The service survives reboot; uninstall removes it
- [ ] Contract suite passes at the lock screen, login screen and a UAC prompt
- [ ] Ctrl+Alt+Del works (document the software SAS policy requirement)
- [ ] `unlock` works at the lock and login screens
