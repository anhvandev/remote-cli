# 03: Connect with Device ID + Device Password from the CLI

**What to build:** Tracer bullet for the TeamViewer-style flow. At registration the Agent generates a Device Password (10 characters, no look-alikes) and keeps it in a Secret Store (this ticket: the encrypted-file fallback with a root-only key; OS stores come with the platform tickets). The Server keeps only a hash. The running Agent opens a local IPC socket. `rcli agent info` shows the Device ID and connection status to any local user, and `sudo rcli agent password show` shows the password to root only. On the Controller side, `rcli setup` saves the Server Address and Server Key (and pins the update key) in the per-user config. `rcli connect <Device ID>` asks for the Device Password once and remembers it in the OS keychain (file fallback). After that every Device command takes just the Device ID, and `rcli forget <Device ID>` removes the password. A password connection reaches only its own Device and no admin endpoint. It is named `<controller hostname> (<IP>)` in audit entries and as Control Lock holder. Server roles stop gating anything that runs on a Device (Device Settings take over in ticket 07). See `.scratch/easy-setup/spec.md`, ADR 0003/0004.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Fresh Controller: `rcli setup`, `rcli connect <ID>`, then `rcli shot <ID>` works with no env vars
- [ ] A wrong password at `connect` is reported and not saved
- [ ] A password connection gets an error for any other Device and for admin endpoints
- [ ] Audit and Control Lock holder show `<hostname> (<IP>)`
- [ ] `rcli agent info` works for any local user; `password show` is refused to non-root
- [ ] The password is never stored in plaintext on the Device, the Server or the Controller config file
- [ ] CLI tests use a per-test config directory and a file-backed keychain
