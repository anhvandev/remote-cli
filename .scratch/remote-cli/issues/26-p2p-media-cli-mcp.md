# 26: Peer-to-peer media for CLI and MCP

**What to build:** Large Observations and clips requested from the CLI or MCP travel over WebRTC peer-to-peer (TURN fallback) instead of through the Server connection, cutting latency and VPS bandwidth. Small messages keep using the Server connection.

**Blocked by:** 21, 09

**Status:** resolved

- [ ] CLI/MCP negotiate WebRTC via Server signaling for large media
- [ ] Falls back to TURN, then to Server relay, without the Controller noticing
- [ ] Results are identical whichever path was used

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
