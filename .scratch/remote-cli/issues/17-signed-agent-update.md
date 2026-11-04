# 17: Signed Agent auto-update

**What to build:** Admins publish Agent builds to the Server and roll them out fleet-wide or to selected Devices. Agents download, verify the signature, install and restart. Unsigned or wrongly signed builds are rejected.

**Blocked by:** 02

**Status:** resolved

- [ ] Admin uploads a signed build and targets all or selected Devices
- [ ] Agent verifies the signature before installing; bad signature → rejected and reported
- [ ] Device reports its Agent version
- [ ] Rollout is visible per Device

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
