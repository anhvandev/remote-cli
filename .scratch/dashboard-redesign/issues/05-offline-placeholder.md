# 05: Offline and Waking message on Màn hình and Terminal

**What to build:** When a Device is Offline, Màn hình and Terminal show, instead of their content, a message that the Device is Offline with a Wake button (the existing power action). When it is Waking, they say they are waiting for the Device to come Online. Tabs are never hidden or disabled; Tổng quan and Lịch sử work as usual. See spec (Màn hình and Terminal stories) and Implementation Decisions (Offline/Waking placeholder).

**Blocked by:** 03

**Status:** resolved

- [ ] Offline Device: Màn hình and Terminal show the Offline message and a Wake button, no Live View or terminal
- [ ] Waking Device: both show "Đang chờ Device Online…"
- [ ] When the Device comes Online the normal content appears (on the next refresh, as the rest of the page does)
- [ ] Playwright: an Offline Device's Màn hình and Terminal show the message; Wake moves the Device to Waking

## Comments

- 2026-11-09: Resolved on integration branch `feat/dashboard-redesign` (tip after review fixes: 45fa22e). Code review findings fixed on branch `review-fixes`. Playwright suite: 32 passed.
