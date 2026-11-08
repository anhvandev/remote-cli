# 03: Device page split into four tabs

**What to build:** Opening a Device shows a shared header (name, Device Status, power buttons) and a tab bar with **Tổng quan**, **Màn hình**, **Terminal**, **Lịch sử**, each at its own URL. Tổng quan (the Device's root URL, where clicking a Device lands) holds current resources, screen state and hardware inventory. Màn hình holds the Live View and the Control Lock card. Terminal holds the terminal. Lịch sử holds the resource history charts with the existing range choices. The Control Lock provider lives in the shared Device layout so tab switches don't drop the lock watch. Leaving Màn hình stops the Live View and leaving Terminal closes the terminal session, as today. See spec (Device stories) and Implementation Decisions (Device layout, Session lifetime).

**Blocked by:** 01

**Status:** resolved

- [ ] Shared Device layout fetches the Device, its Control Lock and the current User once and renders header + tab bar on every tab
- [ ] Tabs are links with their visible names; the active one has `aria-current="page"`
- [ ] Tổng quan is the Device's root URL; Màn hình, Terminal, Lịch sử are sub-routes
- [ ] Lịch sử keeps its range query parameter
- [ ] Control Lock is not released by switching tabs
- [ ] An unknown Device still shows the not-found page
- [ ] Existing specs for live view, direct control, terminal, history and power/wake updated to open the right tab, and passing
- [ ] Playwright: clicking a Device lands on Tổng quan; each tab opens its own URL and is marked active

## Comments

- 2026-11-09: Resolved on integration branch `feat/dashboard-redesign` (tip after review fixes: 45fa22e). Code review findings fixed on branch `review-fixes`. Playwright suite: 32 passed.
