# 07: Quản trị overview with four cards

**What to build:** The Quản trị root shows four cards, each leading to its sub-page. **User:** number of Admins and Operators, disabled Users, API tokens. **Device:** Online/Offline/Waking counts, revoked Devices, Devices without an unlock password. **Cập nhật Agent:** for each version being rolled out, Devices updated out of the total and how many failed; failed Devices listed by name with their error, linking to that Device's Tổng quan. **Enrollment Code:** a "Tạo Enrollment Code" action leading to its sub-page (no count; there is no API to list codes). Uses only data the old Quản trị page already fetched; no charts. See spec (Quản trị: overview).

**Blocked by:** 06

**Status:** resolved

- [ ] Four cards with the counts above, computed from existing Controller API data
- [ ] Rollout progress grouped by version as the old page did
- [ ] Failed Agent updates listed with Device name and error, each linking to that Device's Tổng quan
- [ ] Each card links to its sub-page
- [ ] Playwright: counts match the devstack fixtures; a failed update links to its Device

## Comments

- 2026-11-09: Resolved on integration branch `feat/dashboard-redesign` (tip after review fixes: 45fa22e). Code review findings fixed on branch `review-fixes`. Playwright suite: 32 passed.
