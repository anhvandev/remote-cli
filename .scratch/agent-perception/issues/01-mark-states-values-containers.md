# 01: Marks carry Mark States, a value and their container, end to end

**What to build:**
- A Mark gains three optional fields, carried from the Agent's accessibility element through the protocol, the API JSON, the MCP result and the CLI Mark table:
  - **Mark States:** only those that differ from the default, drawn from disabled, focused, checked, selected, expanded and collapsed.
  - **value:** kept apart from the label `text`.
  - **parent:** the number of the nearest container Mark above it.
- Four container roles join the existing ten: window, dialog, menu, list. Containers are ordinary Marks: numbered, drawn on the annotated image, and targetable by number or text.
- A password element never carries a value, whatever the platform adapter reports.
- Marks from OCR carry none of the new fields.
- This ticket works on FakePlatform. Real OS adapters fill the fields in tickets 02–04.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] With a scripted tree holding a ticked checkbox, a disabled button, a filled field, a password field and a dialog containing buttons, the actions API with `marks: true` returns:
  - the right `states`, `value` and `parent`;
  - empty fields omitted;
  - no value on the password field.
- [ ] The dialog container is a numbered Mark on the annotated image and can be targeted by text.
- [ ] The MCP `observe` result carries the same fields, and the tool schemas describe them.
- [ ] The CLI Mark table shows Mark State, value (shortened when long) and container columns. `--json` carries the full value.
- [ ] OCR Marks (no tree) are unchanged.
- [ ] `docs/usage.md` describes Mark States, values and containers.
- [ ] Tests extend the existing Marks test file.
