# 21: Live view over WebRTC

**What to build:** People can watch a Device's screen live in the Dashboard. Media flows over WebRTC peer-to-peer with the Server as signaling, and falls back to coturn (added to Docker Compose) when NAT blocks a direct path.

**Blocked by:** 19, 05

**Status:** resolved

- [ ] Server provides WebRTC signaling; coturn in Docker Compose
- [ ] Live stream works peer-to-peer and through TURN fallback
- [ ] Live view does not require the Control Lock
- [ ] Control indicator shows on the Device while a live stream is active
- [ ] Playwright test opens live view against the FakePlatform

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
