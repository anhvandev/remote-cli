# 11: Admin Device actions on the Dashboard: ID, name, new password

**What to build:** On a Device's page, Admins can change the Device ID (9 digits, unique), rename it, and set a new Device Password, which is pushed to the Device's Secret Store. Admins never see the current password. This is how headless Devices are managed remotely. See `.scratch/easy-setup/spec.md` (Dashboard).

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] Admin can change ID (duplicate refused), name and set a new password; the Device picks them up live
- [ ] Operators do not see these actions
- [ ] Playwright test covers them
