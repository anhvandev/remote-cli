# 08: Device Settings: accept each connection, see who is controlling, disconnect

**What to build:** `sudo rcli agent accept password|password+accept` sets whether a person at the Device must accept each new connection (prompt with Accept/Reject, auto-reject after 30 s, rejected when no session can show it). `rcli agent info` lists who is currently connected, for example `dev-laptop (10.0.0.5)` or `alice (Dashboard)`. `rcli agent disconnect [<who>]` is allowed for any local user and ends that Controller's sessions (Live View, shell, Control Lock). The Controller can reconnect if it still has the password. Applies equally to Dashboard Users. Disconnects and prompt answers are audited. See `.scratch/easy-setup/spec.md`, ADR 0004.

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] password+accept refuses until accepted; times out to reject
- [ ] `info` lists current Controllers by name
- [ ] Disconnect ends Live View and shell and releases the Control Lock, without root
- [ ] Dashboard Users are subject to the same rules
- [ ] Disconnects are audited
