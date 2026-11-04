# 08: Action Batch and `frames` Observation

**What to build:** A Controller sends an Action Batch (ordered Actions plus waits) in one request and gets one Observation back. The `frames` mode returns change-triggered, timestamped frames captured during the Action or Action Batch, capped at a maximum count, so an AI can follow continuous interactions without video.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Action Batch runs Actions in order; a failing Action stops the batch and reports which step failed
- [ ] Waits inside a batch are supported
- [ ] `frames` captures only when the screen changes noticeably, with timestamps, capped at a configurable maximum
- [ ] Each Action in a batch is audited
- [ ] CLI accepts a batch from JSON input
