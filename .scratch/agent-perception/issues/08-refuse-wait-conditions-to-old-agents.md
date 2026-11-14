# 08: The Server refuses Wait Conditions aimed at older Agents

**What to build:**
- An Agent that predates Wait Conditions would silently ignore `until` and answer at once, and a Controller would take that for a met condition.
- The Server checks the Device's reported Agent version. It refuses any request carrying a Wait Condition, in `observe` or in a batch step, when that version is below the first one supporting Wait Conditions. The error is `agent_too_old` and names the version needed.
- Requests without a Wait Condition are not affected.
- The testkit gains an option to start an Agent that reports an older version.

**Blocked by:** 05

**Status:** resolved

- [x] An Agent started through the new testkit option reports the older version in the Device JSON.
- [x] A Wait Condition sent to it is refused with `agent_too_old`, and the message names the version needed.
- [x] A plain observe and a batch without a Wait Condition still succeed on it.

## Comments

- 2026-11-14: Implemented on `agent-perception`; review fixes in a293522.
