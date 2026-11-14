# 03: macOS reads Mark States, values and containers from the focused window

**What to build:**
- **Fields:** the macOS adapter reads each node's attributes in one batched call: role, subrole, title, description, placeholder, value, enabled, focused, selected and expanded. From these it fills the new Mark fields.
  - `text` is the label only. `AXValue` no longer stands in for `text`.
  - A secure text field never reports a value.
  - Window, sheet/dialog, menu and list map to the container roles, and parent links come from the walk.
- **Scope:** the focused window plus the open menus and sheets of the focused app, instead of every window of the app. The node limit and messaging timeout stay as they are.
- Reference: Peekaboo and agent-desktop attribute lists (`docs/research/computer-use-source-verification.md`, §3 and §2, upgrade 1).

**Blocked by:** 01

**Status:** ready-for-human

- [ ] Checklist on a real macOS Device, with results appended under `## Comments`:
  - [ ] a ticked checkbox shows `checked`, and a disabled button shows `disabled`;
  - [ ] a text field shows its value in `value`, not in `text`, and a password field shows no value;
  - [ ] the buttons of a sheet or dialog name it as `parent`;
  - [ ] an open menu gets Marks;
  - [ ] a second window of the same app, behind the focused one, adds no Marks.
- [x] `docs/usage.md` notes the behaviour change: a field whose only label was its value now has an empty `text`.

## Comments

- 2026-11-14: Adapter code done on `agent-perception` (compiles for this OS; not run on a real Device). Open: the real-Device checklist above, results to be appended here.
