# 09: `clip` Observation

**What to build:** A Controller can ask for a `clip` Observation: a short H.264 recording of the Action or Action Batch, for human review or video-capable models.

**Blocked by:** 05

**Status:** resolved

- [ ] `clip` returns an H.264 clip covering the Action duration plus settle time
- [ ] Clip length is capped
- [ ] Clip is relayed, never stored

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
