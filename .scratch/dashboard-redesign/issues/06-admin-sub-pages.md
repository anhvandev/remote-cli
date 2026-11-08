# 06: Quản trị split into four sub-pages

**What to build:** The single Quản trị page becomes four sub-pages: **User** (Users and API tokens), **Device** (rename, merge history, revoke, and unlock password per Device), **Enrollment Code**, **Cập nhật Agent** (upload a build, roll out, rollout progress). Forms and server actions move unchanged. The sidebar shows the four sub-entries under Quản trị for Admins. The Admin-only guard moves to the Quản trị layout so it covers every sub-page. Until ticket 07, the Quản trị root just links to the four sub-pages. See spec (Quản trị: sub-pages) and Implementation Decisions (Admin sub-pages, Admin-only guard).

**Blocked by:** 01

**Status:** resolved

- [ ] Four sub-pages under Quản trị, each with the same sections (region labels unchanged) and behaviour as today
- [ ] Unlock password section lives on the Device sub-page
- [ ] Sidebar shows four sub-entries under Quản trị for Admins only; the current one is marked
- [ ] Operators opening any Quản trị URL see only the "Chỉ admin" message and no admin data
- [ ] Quản trị root links to the four sub-pages
- [ ] Admin spec updated to reach each section through its sub-page, and passing
- [ ] Playwright: an Operator opening each sub-page URL sees only the Admin-only message

## Comments

- 2026-11-09: Resolved on integration branch `feat/dashboard-redesign` (tip after review fixes: 45fa22e). Code review findings fixed on branch `review-fixes`. Playwright suite: 32 passed.
