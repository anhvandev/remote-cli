# 09: `clip` Observation

**What to build:** A Controller can ask for a `clip` Observation: a short H.264 recording of the Action or Action Batch, for human review or video-capable models.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] `clip` returns an H.264 clip covering the Action duration plus settle time
- [ ] Clip length is capped
- [ ] Clip is relayed, never stored
