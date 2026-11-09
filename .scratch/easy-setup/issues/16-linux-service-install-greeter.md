# 16: Linux: root service, install/uninstall, greeter, deb/rpm

**What to build:** Replaces `.scratch/remote-cli/issues/33`. `sudo rcli agent install --server … --key … [--allow-root]` copies the binary, records the Terminal User (`SUDO_USER`), registers the Device, and installs and starts a root systemd service. `sudo rcli agent uninstall` stops and removes the service, removes the Device on the Server and deletes local data. The service finds the seat0 greeter's X server and auth file and runs the helper as the greeter user, so the lock screen and display-manager login can be observed and controlled. The Secret Store uses `systemd-creds` (TPM when present). deb/rpm packages run the same install. Works on headless servers. See `.scratch/easy-setup/spec.md`.

**Blocked by:** 15, 06

**Status:** ready-for-agent

- [ ] Install, reboot (service comes back) and uninstall verified on a real desktop and a headless server
- [ ] Contract suite passes at the lock screen and at the GDM/LightDM/SDDM greeter
- [ ] `unlock` works at both
- [ ] deb/rpm install and remove cleanly
