# 07: Wait until a Mark's value or Mark State changes

**What to build:**
- The Wait Condition gains two conditions on the Mark named by `text` and optional `role`:
  - `value` with `equals` or `contains`;
  - `state` with `has` or `lacks` a Mark State.
- Both work in `observe` and in the `wait` step of a batch.
- **When they fail at once:**
  - the screen has no accessibility tree: `no_accessibility_tree`, because OCR never reports values or Mark States;
  - more than one Mark matches: `ambiguous_text`, listing the matches as Action targeting does.
- MCP schemas, CLI flags and `docs/usage.md` cover both conditions.

**Blocked by:** 01, 05

**Status:** resolved

- [x] `state has checked` is met when the test ticks the checkbox in FakePlatform's tree during the wait.
- [x] `state lacks disabled` is met when the test enables the button.
- [x] `value equals "An"` and `value contains "An"` are met when the field's value changes.
- [x] With no accessibility tree, a `value` condition fails at once with `no_accessibility_tree`.
- [x] Two Marks matching the text fail with `ambiguous_text`. Adding `role` narrows the match to one.

## Comments

- 2026-11-14: Implemented on `agent-perception`; review fixes in a293522.
