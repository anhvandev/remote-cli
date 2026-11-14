# Agent perception: Marks with Mark States, values and containers, plus Wait Conditions

**Status:** ready-for-agent

## Problem Statement

An AI Controller driving a Device through `rcli mcp` or the CLI sees too little to tell what really happened on screen. It has to guess from screenshots.

- A Mark today is only a number, a role, a text and a box. A Controller cannot tell whether a checkbox is ticked, a button is disabled, a field holds the right value, or which dialog a button belongs to. Checking "the setting is now on" means reading pixels and hoping.
- On Windows only the foreground window is read. A context menu, dropdown or child dialog that opens in its own window has no Marks at all, so the Controller falls back to coordinates. On macOS the whole focused app is walked, which is slow and includes windows the user cannot see.
- There is no way to wait for something to happen. A Controller can only wait a fixed time (at most 5 s per request) and then look again. It either waits too long, or acts before the app is ready and has to poll by sending requests over and over.
- On Windows, reading the accessibility tree has no node limit and no timeout. One hung app can stall the request.

## Solution

- **Richer Marks.** Each Mark now also carries:
  - its **Mark States** that differ from the default: disabled, focused, checked, selected, expanded, collapsed;
  - its **value**, kept apart from its label text and never filled for a password field;
  - the **container** it sits in.

  Containers (window, dialog, menu, list) become Marks too, so "the OK button in the 'Save changes' dialog" is something a Controller can read and say. The same fields appear in the API JSON, the MCP result and the CLI Mark table.
- **Wait Conditions.** A Controller can ask the Agent to wait until something is true on screen, then get the Observation:
  - text, or a Mark, **appears** or **disappears**;
  - a Mark's **value** equals or contains a string;
  - a Mark has or lacks a **Mark State**.

  This works in two places:
  - in `observe`, which needs no Control Lock, so watching does not need the lock;
  - as a step in an Action Batch, e.g. click Save → wait until "Saved" appears → screenshot, all in one request.

  When the time runs out, the request fails with `wait_timeout` and still returns the Observation, so the Controller sees the screen as it was. Waiting for text works at the lock and login screens too, because Marks fall back to OCR there.
- **Better tree coverage.**
  - **Windows:** reads the foreground window plus the popups, menus and dialogs of the same app, with a node limit and timeouts.
  - **macOS:** reads the focused window and its open menus and sheets instead of the whole app.
  - **Linux:** keeps reading every app, as today.
- **Safe with older Agents.** The Server refuses a Wait Condition aimed at an Agent too old to understand it, with `agent_too_old`. Otherwise an old Agent would silently ignore it and answer at once.

## User Stories

1. As an AI Controller, I want each Mark to list its Mark States, so that I can tell whether a checkbox is ticked without reading pixels.
2. As an AI Controller, I want a disabled button to be marked `disabled`, so that I do not click something that cannot respond.
3. As an AI Controller, I want to see which Mark has keyboard focus, so that I know where typed text will go.
4. As an AI Controller, I want selected list items and tabs marked `selected`, so that I can confirm my choice took effect.
5. As an AI Controller, I want tree items, combo boxes and menus marked `expanded` or `collapsed`, so that I know whether I must open them first.
6. As an AI Controller, I want only non-default Mark States listed, so that the Mark table stays short and cheap in tokens.
7. As an AI Controller, I want a field's value kept separate from its label, so that I can check "the Name field holds 'An'" exactly.
8. As an AI Controller, I want sliders and spinners to report their value, so that I can verify a numeric setting.
9. As a person whose Device is controlled, I want password field values never reported, so that a secret on screen never leaves the Device in a Mark table or an audit entry.
10. As an AI Controller, I want windows, dialogs, menus and lists to appear as Marks, so that I can see how the screen is structured.
11. As an AI Controller, I want each Mark to name its container Mark, so that I can tell two "OK" buttons apart by the dialog they sit in.
12. As an AI Controller, I want container Marks numbered and drawn on the annotated image like any other Mark, so that the image and the table agree.
13. As an AI Controller on Windows, I want a context menu or dropdown that opens in its own window to have Marks, so that I can pick a menu item by text instead of coordinates.
14. As an AI Controller on Windows, I want a modal dialog of the foreground app to be read even when it is not the foreground window, so that I see the dialog that is blocking the app.
15. As an AI Controller on macOS, I want Marks from the focused window and its open menus and sheets, so that windows hidden behind it do not clutter the table.
16. As an AI Controller on Linux, I want Marks to keep covering every app, so that nothing I rely on today disappears.
17. As an AI Controller, I want reading the accessibility tree to give up after a time and node limit, so that one hung app cannot stall my request.
18. As an AI Controller, I want to wait until a text appears, so that I act as soon as the app is ready instead of after a guessed delay.
19. As an AI Controller, I want to wait until a text disappears, so that I know a spinner or "Loading…" dialog is gone.
20. As an AI Controller, I want to wait until a Mark's value equals or contains a string, so that I know a field was filled or a status label changed.
21. As an AI Controller, I want to wait until a Mark has or lacks a Mark State, so that I know a button became enabled or a box got ticked.
22. As an AI Controller, I want to name the Mark in a Wait Condition by its text and, if needed, its role, so that the condition still finds it as the screen is re-read and Mark numbers change.
23. As an AI Controller, I want text in a Wait Condition matched the same way as text targets in Actions (case-insensitive, exact before contains), so that I learn one rule.
24. As an AI Controller, I want a Wait Condition to count as met only when two readings in a row agree, so that a flicker does not fool me.
25. As an AI Controller, I want a default timeout of 5 s and the right to ask for up to 30 s, so that slow apps can be waited for.
26. As an AI Controller, I want a timed-out wait to fail with `wait_timeout` and still return the Observation, so that I can see what the screen showed instead.
27. As an AI Controller, I want a Wait Condition inside an Action Batch to stop the batch on timeout and report the failed step, so that I know which step did not happen.
28. As an AI Controller, I want to wait in `observe` without holding the Control Lock, so that I can watch a Device another Controller is driving.
29. As an AI Controller, I want waiting for text to work at the lock and login screens via OCR, so that I can wait for "Welcome" after typing a password.
30. As an AI Controller, I want a value or Mark State Wait Condition to fail at once when the screen has no accessibility tree, so that I do not wait 30 s for something OCR can never report.
31. As an AI Controller, I want a value or Mark State Wait Condition that matches more than one Mark to fail with `ambiguous_text`, so that I narrow it by role instead of getting a wrong answer.
32. As an AI Controller, I want the Server to refuse a Wait Condition aimed at an older Agent with `agent_too_old` and the version needed, so that I never mistake an ignored wait for a met one.
33. As an AI Controller, I want new Mark fields from a newer Agent to be absent, not wrong, on an older Agent, so that mixed fleets keep working.
34. As a CLI user, I want the Mark table to show Mark States, value and container, so that I can read the screen structure in a terminal.
35. As a CLI user, I want long values shortened in the table but complete in `--json`, so that the table stays readable without losing data.
36. As a CLI user, I want to pass a Wait Condition to `rcli shot` and in `rcli act`/`rcli batch` steps, so that scripts get the same waiting as MCP.
37. As an MCP client author, I want the `observe`, `act` and `batch` tool schemas to describe the Wait Condition and the new Mark fields, so that a model learns to use them from the schema alone.
38. As an auditor, I want Wait Conditions recorded in the audit entry like other Action parameters, so that I can see what a Controller waited for.
39. As a maintainer, I want `docs/usage.md` to describe Mark States, values, containers and Wait Conditions, so that users find them.

## Implementation Decisions

- **Mark model.** The Agent's accessibility element and the protocol Mark gain three optional fields:
  - `states`: a list drawn from `disabled, focused, checked, selected, expanded, collapsed`, holding only states that differ from the default;
  - `value`: a string, absent when the element has none or is a password field;
  - `parent`: the number of the nearest container Mark above it.

  JSON and MCP omit a field when it is empty. Marks that come from OCR have none of the three.
- **Container roles.** Four roles join the existing ten: `window`, `dialog`, `menu`, `list`. Containers are Marks: numbered, drawn, and targetable by number or text like any other Mark.
- **Label vs value.** `text` is the label or name only. On macOS, `AXValue` no longer serves as `text`. Targeting by text keeps matching `text` only.
- **Windows tree reading.**
  - The cached UIA request adds:
    - enabled and keyboard-focus properties;
    - is-password;
    - the Value, Toggle, ExpandCollapse, SelectionItem and RangeValue values;
    - LegacyIAccessible state and value.
  - Parent links come from walking the cached tree. One batched request, no extra round-trips per node.
  - Scope: the foreground window plus the visible top-level popup, menu and dialog windows of the same process, with a modal dialog first. No taskbar.
  - Limits: 3000 nodes, as on the other OSes, plus UIA connection and transaction timeouts (about 2 s and 5 s).
- **macOS tree reading.**
  - Each node's attributes are read in one batched call: role, title, description, placeholder, value, enabled, focused, selected, expanded, subrole.
  - A secure text field never reports a value.
  - Scope: the focused window plus open menus and sheets of the focused app.
- **Linux tree reading.** The AT-SPI state set already fetched also yields the checked, enabled, focused, selected, expanded and collapsible bits. Values come from the Value and Text interfaces. A password-text role never reports a value. Scope is unchanged.
- **Wait Condition contract.** One shape, used by `observe` (`until`) and by the existing `wait` Action, which gains an `until` beside its fixed `ms`:
  - **target:** `text`, plus an optional `role`;
  - **condition:** exactly one of
    - `appears`,
    - `disappears`,
    - `value` (with `equals` or `contains`),
    - `state` (with `has` or `lacks` a Mark State);
  - **`timeout_ms`:** default 5000, at most 30000.

  The Agent re-detects Marks about every 100 ms, the same way it does for an Observation, and treats the condition as met after two agreeing readings in a row.
  - `appears` and `disappears` work on OCR Marks.
  - `value` and `state` fail at once with `no_accessibility_tree` when the screen has no tree.
  - `value` and `state` fail with `ambiguous_text` when more than one Mark matches.

  On timeout the Agent answers `wait_timeout` with the Observation of the last reading. In a batch, it also sets `failed_step`.
- **Request limits.** The Server's per-request cap on waits rises from 5 s to 30 s, counting fixed waits and Wait Condition timeouts together. The Server's timeout for an Agent request grows to cover the requested waits.
- **Version gate.** The Server reads the Device's reported Agent version. It refuses any request carrying a Wait Condition when that version is below the first version that supports Wait Conditions, with `agent_too_old` naming the version needed. New Mark fields need no gate.
- **Surfaces.**
  - **API:** the actions endpoint accepts `until` in the request and in `wait` steps.
  - **MCP:** the `observe`, `act` and `batch` schemas describe `until` and the new Mark fields.
  - **CLI:** `rcli shot`, `rcli act` and `rcli batch` accept a Wait Condition. The Mark table gains Mark State, value (shortened) and container columns. `--json` carries everything.
- **Audit.** Wait Conditions are recorded with the other Action parameters. Values never appear in audit beyond what the Mark table already sends to the Controller, and password values never exist.
- **Docs.** `docs/usage.md` gains Mark States, values, containers and Wait Conditions. `GLOSSARY.md` already defines Mark, Mark State and Wait Condition.

## Testing Decisions

- Good tests drive the system from outside: API calls, CLI commands and MCP calls against a real Server and Agent in the testkit, asserting on what a Controller receives (Mark JSON, CLI table, error codes, Observations). They do not call tree-reading or matching functions directly.
- **Seam 1, Rust testkit (`TestSystem`, FakePlatform, MCP client).** Extend the existing Marks test file rather than adding a new one.
  - **Mark fields:** a scripted tree with a ticked checkbox, a disabled button, a filled field, a password field and a dialog container shows the right `states`, `value` and `parent` in API JSON, in MCP output and in the CLI table. The password field has no value.
  - **Wait Conditions:** the test changes FakePlatform's tree while the request is waiting.
    - `appears`, `disappears`, `value` and `state` are each met;
    - a timeout returns `wait_timeout` with the Observation, and `failed_step` in a batch;
    - `observe` with `until` works without the Control Lock;
    - `appears` works on OCR Marks when there is no tree;
    - `value` without a tree fails with `no_accessibility_tree`;
    - an ambiguous match fails with `ambiguous_text`.
  - **Version gate:** a new testkit option starts an Agent that reports an older version. A request with a Wait Condition to it is refused with `agent_too_old`. This option is the only testkit addition.
  - **Limits:** a request whose waits add up to more than 30 s is refused, extending the existing over-limit check.
  - Prior art: the Marks test file (scripted trees, OCR fallback, stale and ambiguous Marks, CLI table), the MCP test file (MCP client against the system), and the rollout test file (Agent version in Device JSON).
- **Seam 2, checklist on real Devices (Windows, macOS, Linux).** FakePlatform cannot exercise UIA, AX or AT-SPI. Each OS ticket carries a short checklist, run on the team's real machines, with results appended to the ticket's comments:
  - a ticked checkbox shows `checked`;
  - a disabled button shows `disabled`;
  - a text field's value shows and a password field's does not;
  - a dialog's buttons name the dialog as `parent`;
  - on Windows, a context menu and a dropdown get Marks;
  - on macOS, a window behind the focused one adds no Marks;
  - a wait for text works at the lock screen.
- No Dashboard e2e: the Dashboard does not show Marks. No unit tests for state mapping or matching that Seam 1 already covers. One test per acceptance criterion.

## Out of Scope

- Semantic actions (UIA Invoke, AXPress, AT-SPI DoAction) and an `effect` result. These belong to a later spec.
- Element references that survive across Observations. Wait Conditions use text and role instead.
- Listing or focusing windows, and launching apps.
- Recording runs, per-step artifacts, and video longer than the current clip.
- `exec` in the user's GUI session on macOS and Linux. The Windows part is a separate bug ticket (ADR 0010).
- Scenario files, runners, pass/fail verdicts and LLM judging. rcli gives Controllers better eyes, and judging stays with the Controller.
- ScreenCaptureKit capture on macOS, terminal/TUI waits over MCP, zooming into a region, mobile.
- Narrowing Linux's tree scope.

## Further Notes

- Background: `docs/research/rcli-improvements.md` (A1, A2, A8) and `docs/research/computer-use-source-verification.md`, which shows how Cua Driver, agent-desktop, Peekaboo and Windows-MCP read state and value and implement waits, with `file:line`. Their code is MIT or Apache-2.0, so it can be reused in AGPL-3.0 rcli with attribution.
- The Windows `exec` bug (commands always run as SYSTEM) is fixed separately under ADR 0010, ahead of this spec.
- The behaviour change for existing users: on macOS, a field whose only label was its value now has an empty `text` and the value in `value`, so targeting it by its value text stops working. On Windows, popups and dialogs add Marks, so Mark numbers on the same screen can shift.
- Defaults chosen while writing this spec (not discussed):
  - `no_accessibility_tree` for value and state waits without a tree;
  - the 30 s cap counts all waits in one request together;
  - `wait` keeps its fixed `ms` beside `until`.
