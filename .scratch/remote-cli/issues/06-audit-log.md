# 06: Audit log

**What to build:** Every Action is recorded with who, when, which Device, its parameters, and for terminal commands the command and exit code. Entries are kept 90 days. Secrets such as typed passwords and `unlock` input are masked. Media is never stored. Later tickets add their Actions to this log.

**Blocked by:** 05

**Status:** resolved

- [x] Every Action from ticket 05 produces an audit entry
- [x] `rcli audit` filters by Device, user and time range (admin only)
- [x] Text typed into password fields and all `unlock` content is masked
- [x] Entries older than 90 days are removed (injectable clock)
- [x] No Observation images or clips are stored

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
