# 16: Device lifecycle: revoke, rename, delete, merge history

**What to build:** Admins can revoke a Device's credential, rename or delete Devices, and merge an old Device's history into a new one after a reinstall. Reinstalled machines are never matched automatically.

**Blocked by:** 02, 04

**Status:** ready-for-agent

- [ ] Revoked Device can no longer connect
- [ ] Rename and delete Devices
- [ ] Merge moves resource history (and keeps audit references) from old Device to new Device
- [ ] Re-enrolling the same machine creates a new Device
