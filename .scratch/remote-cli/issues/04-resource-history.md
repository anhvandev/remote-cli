# 04: Resource history with rollup, retention and backfill

**What to build:** Resource samples are stored as history: raw 5-second samples kept 7 days, 1-minute rollups kept 90 days. While disconnected the Agent buffers up to 1 hour of samples and backfills them on reconnect, so short outages leave no gaps.

**Blocked by:** 03

**Status:** resolved

- [ ] Raw samples stored and queryable by Device and time range via `rcli metrics history`
- [ ] 1-minute rollups produced; raw data older than 7 days and rollups older than 90 days are removed
- [ ] Agent buffers up to 1 hour while disconnected; older buffered samples are dropped first
- [ ] After reconnect the buffered samples appear in history with their original timestamps
- [ ] Tests use the injectable clock for rollup and retention

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
