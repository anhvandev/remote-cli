# 05: First Action with a `shot` Observation

**What to build:** A Controller acquires the Control Lock on a Device, sends a mouse or keyboard Action, and gets back a `shot` Observation of the result. Coordinates are given in Observation image space as (display, x, y). The Agent's Coordinate Mapper converts them to real pixels. Basic acquire/release only; the timeout rules come in ticket 07.

**Blocked by:** 02

**Status:** resolved

- [ ] `rcli lock acquire|release|status <device>`; input Actions without the Control Lock are refused with an error naming the holder
- [ ] Actions: move, click, double-click, right-click, drag, scroll, type text, key combination
- [ ] `shot` waits for the screen to settle (up to ~1 s), downscales (long edge ~1280 px) and compresses (WebP/JPEG)
- [ ] Repeat Observation of an unchanged screen returns "unchanged" instead of an image
- [ ] Coordinates map correctly across displays with different scales (tests with a multi-display FakePlatform)
- [ ] Default Observation is the display last acted on; `--display all` returns one image per display; displays can be listed with resolution and arrangement
- [ ] Observation mode `none` is supported
- [ ] Non-holders can still request a screenshot (observe-only)

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
