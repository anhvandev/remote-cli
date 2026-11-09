# 10: Server Settings: replace the Server Key, set the Server Address

**What to build:** An Admin-only Server Settings page in the Dashboard, plus `rcli server-key rotate` and `rcli server-address set`. Replacing the Server Key changes only its secret, so registered Devices stay Online and new installs and Controllers need the new key. The Server Address replaces `RCLI_AGENT_HOSTNAMES`, is kept in the Server's storage, and reissues the Server's TLS certificate when changed. See `.scratch/easy-setup/spec.md` (Server Settings).

**Blocked by:** 02

**Status:** ready-for-agent

- [x] After a key replacement, registered Devices stay Online and the old key no longer registers or connects
- [x] Changing the Server Address reissues the certificate and Agents reconnect on the new name
- [x] Operators cannot see the page or use the commands
- [x] Playwright test covers the page
