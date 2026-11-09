# 06: Change Device Password, ID, name and Server from the Device

**What to build:** Extend the local IPC with root-only `rcli agent password set | id set | name set | server set`. Each applies to the running service at once, without a restart. Password, ID and name changes go to the Server: the ID is checked for uniqueness, and a new password must be at least 8 characters. Existing sessions survive a password change. Changing the Server Address or Key reconnects the Agent. See `.scratch/easy-setup/spec.md` (Local IPC).

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] Each command applies live; tests drive them with `rcli agent …` in-process
- [ ] Non-root callers are refused every change
- [ ] A duplicate Device ID is refused with a clear error; Controllers using the old ID get a clear 'not found'
- [ ] After a password change new connections need the new password and open sessions continue
- [ ] Changing Server Address/Key reconnects without a restart
