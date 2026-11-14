# 02: Windows reads Mark States, values, containers, and popups, menus and dialogs

**What to build:**
- **Fields:** the Windows adapter fills the new Mark fields from UIA in the same batched cached request. It reads:
  - enabled and keyboard focus;
  - is-password;
  - the Value, Toggle, ExpandCollapse, SelectionItem and RangeValue values;
  - LegacyIAccessible state and value.

  Parent links come from the cached tree. Window, dialog, menu and list map to the container roles.
- **Scope:** the foreground window plus the visible top-level popup, menu and dialog windows of the same process. A modal dialog comes first. The taskbar is not read.
- **Limits:** 3000 nodes, plus UIA connection and transaction timeouts (about 2 s and 5 s), so a hung app cannot stall a request.
- Reference: how agent-desktop, Windows-MCP and Cua Driver read these properties and scopes (`docs/research/computer-use-source-verification.md`, §2, §4, §1 and upgrades 1 and 8).

**Blocked by:** 01

**Status:** ready-for-human

- [x] A password field never reports a value.
- [ ] A tree larger than 3000 nodes, or an app that does not answer, still returns an Observation within the timeouts.
- [ ] Checklist on a real Windows Device, with results appended under `## Comments`:
  - [ ] a ticked checkbox shows `checked`, and a disabled button shows `disabled`;
  - [ ] a text field shows its value, and a password field does not;
  - [ ] the buttons of a dialog name the dialog as `parent`;
  - [ ] a right-click context menu and an open dropdown get Marks;
  - [ ] a modal dialog of the foreground app gets Marks;
  - [ ] the lock screen still falls back to OCR Marks.

## Comments

- 2026-11-14: Adapter code done on `agent-perception` (compiles for this OS; not run on a real Device). Open: the real-Device checklist above, results to be appended here.
