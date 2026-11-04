# 06: Audit log

**What to build:** Every Action is recorded with who, when, which Device, its parameters, and for terminal commands the command and exit code. Entries are kept 90 days. Secrets such as typed passwords and `unlock` input are masked. Media is never stored. Later tickets add their Actions to this log.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Every Action from ticket 05 produces an audit entry
- [ ] `rcli audit` filters by Device, user and time range (admin only)
- [ ] Text typed into password fields and all `unlock` content is masked
- [ ] Entries older than 90 days are removed (injectable clock)
- [ ] No Observation images or clips are stored
