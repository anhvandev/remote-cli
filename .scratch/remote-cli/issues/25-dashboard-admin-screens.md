# 25: Dashboard: admin screens

**What to build:** Admins manage users and tokens, Enrollment Codes, Devices (revoke, rename, delete, merge), unlock passwords and Agent updates from the Dashboard.

**Blocked by:** 19, 15, 16, 17

**Status:** resolved

- [ ] Users/tokens, Enrollment Codes, Device lifecycle, unlock passwords (write-only), update rollout
- [ ] Admin-only; operators don't see these screens
- [ ] Unlock passwords can be set or cleared but never displayed

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
