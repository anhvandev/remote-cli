# 16: Device lifecycle: revoke, rename, delete, merge history

**What to build:** Admins can revoke a Device's credential, rename or delete Devices, and merge an old Device's history into a new one after a reinstall. Reinstalled machines are never matched automatically.

**Blocked by:** 02, 04

**Status:** resolved

- [ ] Revoked Device can no longer connect
- [ ] Rename and delete Devices
- [ ] Merge moves resource history (and keeps audit references) from old Device to new Device
- [ ] Re-enrolling the same machine creates a new Device

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
