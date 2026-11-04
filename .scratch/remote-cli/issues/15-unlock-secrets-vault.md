# 15: `unlock` via the Secrets Vault

**What to build:** An admin stores a Device's login password, encrypted with a server master key. A Controller sends `unlock`, and the Agent types the password at the lock or login screen. The password never leaves the Server except to that Agent.

**Blocked by:** 05, 06

**Status:** ready-for-agent

- [ ] Admin sets/clears a Device's unlock password via CLI; it is stored encrypted
- [ ] `rcli unlock <device>` makes the Agent type the password; requires the Control Lock
- [ ] The password never appears in any Controller API response, CLI output or audit entry
- [ ] Clear error when no password is stored
