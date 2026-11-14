# 06: A Wait Condition as a step in an Action Batch

**What to build:**
- The existing `wait` Action gains an optional `until` (the Wait Condition from 05) beside its fixed `ms`. A Controller can send click Save → wait until "Saved" appears → observe as one Action Batch.
- On timeout the batch stops at that step. The answer carries `wait_timeout`, `failed_step` and the Observation of the last reading.
- The 30 s per-request cap counts fixed waits and Wait Condition timeouts together.
- Wait Conditions are recorded in the audit entry with the other Action parameters.
- **Surfaces:**
  - CLI: `rcli act` and `rcli batch` accept the step;
  - MCP: `act` and `batch` describe it in their schemas.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] A batch "click → wait until text appears → observe" succeeds when the test shows the text after the click.
- [ ] A step that times out stops the batch, and later steps do not run. The answer has `wait_timeout`, `failed_step` pointing at the wait, and an Observation.
- [ ] A batch whose fixed waits and timeouts add up to more than 30 s is refused, extending the existing over-limit check.
- [ ] The audit entry for the batch shows the Wait Condition.
- [ ] `rcli batch` and the MCP `batch` tool accept the step.
