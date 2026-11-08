# 04: Control Lock reminder on every Device tab

**What to build:** While the current User holds a Device's Control Lock, the Device header shows "Bạn đang giữ Control Lock · Nhả" on every tab, and the User can release the lock from it without going back to Màn hình. Control Lock rules are unchanged: switching tabs never releases it. See spec (Device: shared header and tabs).

**Blocked by:** 03

**Status:** resolved

- [ ] Reminder appears in the header only while the current User holds the Control Lock
- [ ] Reminder stays visible after switching to any other tab
- [ ] "Nhả" in the reminder releases the lock; the reminder disappears and Màn hình shows the lock as free
- [ ] Playwright: take the lock on Màn hình, switch to Tổng quan, see the reminder and the lock still held; release from the reminder frees it

## Comments

- 2026-11-09: Resolved on integration branch `feat/dashboard-redesign` (tip after review fixes: 45fa22e). Code review findings fixed on branch `review-fixes`. Playwright suite: 32 passed.
