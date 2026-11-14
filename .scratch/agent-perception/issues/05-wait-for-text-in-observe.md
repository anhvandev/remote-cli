# 05: Wait until text appears or disappears, in `observe`

**What to build:**
- A Controller can add a Wait Condition (`until`) to an observe request: `appears` or `disappears`, naming its target by `text` plus an optional `role`.
  - Text matches the same way as Action text targets: case-insensitive, exact before contains.
  - `timeout_ms` defaults to 5000, at most 30000.
- **How the Agent waits:**
  - It re-detects Marks about every 100 ms, the way it does for an Observation, and answers once two readings in a row meet the condition.
  - On timeout it answers `wait_timeout` with the Observation of the last reading.
  - It works on OCR Marks, so it works at the lock and login screens.
  - Observing needs no Control Lock, so neither does waiting.
- **Server:** the per-request cap on waits rises from 5 s to 30 s. The Server's timeout for the Agent request grows to cover the requested wait.
- **Surfaces:**
  - API: the actions endpoint accepts `until`;
  - MCP: `observe` describes `until` in its schema;
  - CLI: `rcli shot` takes a Wait Condition.

**Blocked by:** None (can start immediately)

**Status:** ready-for-human

- [x] A Controller without the Control Lock observes with `until: appears "Saved"`. The test adds that text to FakePlatform's tree during the wait, and the Observation comes back showing it.
- [x] `disappears` is met when the test removes the text.
- [x] A condition never met returns `wait_timeout` with an Observation, after about `timeout_ms`.
- [x] With no accessibility tree, `appears` is met from OCR text.
- [x] A `timeout_ms` above 30000 is refused as invalid.
- [x] The MCP `observe` tool and `rcli shot` accept the Wait Condition.
- [x] `docs/usage.md` describes Wait Conditions.
- [ ] Checklist on one real Device, with the result appended under `## Comments`: at the lock screen, typing the password with a `secret` type Action and then waiting for a desktop text succeeds.

## Comments

- 2026-11-14: Code and tests done on `agent-perception`. Open: the real-Device lock-screen checklist item (needs a human on a real Device).
