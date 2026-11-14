# 04: Linux reads Mark States, values and containers from AT-SPI

**What to build:**
- **Mark States:** the Linux adapter keeps the checked, enabled, focused, selected, expanded and collapsible bits from the AT-SPI state set it already fetches.
- **Values:** read from the Value and Text interfaces. A password-text role never reports a value.
- **Containers:** window/frame, dialog, menu and list map to the container roles. Parent links come from the walk.
- **Scope:** unchanged (every app), with the same node limit and timeout.
- Reference: agent-desktop and Cua Driver AT-SPI readers (`docs/research/computer-use-source-verification.md`, §2 and §1, upgrade 1).

**Blocked by:** 01

**Status:** ready-for-human

- [ ] Checklist on a real Linux Device (X11 or Wayland), with results appended under `## Comments`:
  - [ ] a ticked checkbox shows `checked`, and a disabled button shows `disabled`;
  - [ ] a text field shows its value, and a password field does not;
  - [ ] the buttons of a dialog name the dialog as `parent`;
  - [ ] Marks still cover every visible app.

## Comments

- 2026-11-14: Adapter code done on `agent-perception` (compiles for this OS; not run on a real Device). Open: the real-Device checklist above, results to be appended here.
