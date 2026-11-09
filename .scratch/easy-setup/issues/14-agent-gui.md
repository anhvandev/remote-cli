# 14: Agent GUI window

**What to build:** One small egui window that is a client of the local IPC. It shows the Device ID; reveals, copies and changes the Device Password after an OS elevation prompt (polkit, UAC, macOS authorization); shows connection status and Server Address/Key; edits Device Settings (Terminal User, root level, accept mode) and the unlock password; shows 'being controlled by …' with Disconnect and 'grant root to this session'; and shows the prompts from tickets 07 and 08. No tray icon. It opens from the app menu or when `rcli` is run with no arguments on a desktop, and the service opens it when someone starts controlling the Device. The binary must still start on machines without display libraries. This window is the control indicator for the platform tickets. See `.scratch/easy-setup/spec.md` (GUI).

**Blocked by:** 07, 08, 09

**Status:** ready-for-agent

- [ ] Manual checklist on a Linux desktop: every item above works and applies live
- [ ] Password reveal needs elevation; Disconnect does not
- [x] `rcli agent …` still works on a headless machine with no GUI libraries

## Comments

- Implemented on branch `t/14` (3 commits, subagent). The window is `crates/cli/src/gui.rs`, behind a new `gui`
  feature on the `rcli` crate (optional `eframe`, the egui windowing crate; it re-exports `egui`). The default
  build links no display libraries at all (`cargo tree -p rcli -e normal`: no eframe/winit/egui/glow).
- **What the window shows:** Device ID, connection status, Server Address, the Device Settings (Terminal User,
  root level, accept mode), the Device Password with Reveal / Copy / Change, and "Being controlled by" with a
  Disconnect button per connected Controller. It refreshes over the Agent's local IPC every 2 s, so changes
  made elsewhere (`sudo rcli agent root …`) show up live. `rcli agent info` now also returns `settings`
  (`terminal_user`, `root`, `accept`) — any local user may see (not change) them.
- **Elevation.** Reveal and Change are root-only IPC calls. When the Agent refuses only for lack of root, the
  GUI asks the OS to run `rcli agent password show|set` elevated: `pkexec` on Linux (the polkit prompt),
  `osascript -e 'do shell script … with administrator privileges'` on macOS. The new password travels over
  stdin (the pkexec pipe) or a 0600 temp file deleted right after (`do shell script` has no stdin) — never on a
  command line, so it does not leak into `ps`. For that reason `rcli agent password set` reads one line from
  stdin when `--password` is omitted. Disconnect needs no elevation (any local user may end a Controller).
- **Launch.** `rcli` with no arguments on a desktop opens the window (`RCLI_AGENT_SOCKET` overrides the socket).
  Without the `gui` feature the CLI is unchanged and `rcli agent run` is headless as before.
- **Manual checklist (Linux desktop).** Build `cargo build -p rcli --features gui` with an Agent installed
  (ticket 16's `rcli agent install`, or `rcli agent run --server … --key …`), then:
  - [ ] `rcli` opens one window: Device ID, connection status, Server Address and Device Settings all match
        `rcli agent info --json`.
  - [ ] Connect a Controller (`rcli connect <id>` or the Dashboard): "Being controlled by" names it within 2 s.
  - [ ] Click its Disconnect: the session ends and the line disappears — with no elevation prompt.
  - [ ] Reveal: the polkit prompt appears; only after authenticating is the password shown. Cancel the prompt:
        the error is shown and no password appears. Copy puts the password on the clipboard.
  - [ ] Change: a password under 8 characters is refused by the Agent with its message; a valid one asks polkit
        and then applies (`sudo rcli agent password show` confirms; open sessions keep working).
  - [ ] `sudo rcli agent root never` in a terminal: the window shows `Root: never` within 2 s (applies live).
  - [ ] Stop the Agent and run `rcli`: the window says `Agent unreachable: cannot reach the Agent at …`.
  - [ ] On a machine with no display, the default build (`cargo build -p rcli`): `rcli agent run`, `info`,
        `password …` work; `rcli` with no arguments prints the CLI error as before.
- Not verified here: the window rendering, the polkit prompt and the clipboard (no display session was used —
  everything above is compile-checked only); nothing on the manual checklist was run.
- Not implemented yet (the rest of the ticket's items; the IPC commands for them all exist): editing Device
  Settings and the unlock password (the GUI displays settings but has no editor — add widgets over
  `terminal-user.set`, `root.set`, `accept.set`, `unlock-password.set`), showing the Server Key (root-only
  read, deliberately not in `info`), the ticket 07/08 prompts in the window and "grant root to this session"
  (`root.grant`), the service opening the window when someone starts controlling the Device, the app-menu
  entry, and the macOS permission guide. Windows UAC elevation is not implemented (ticket 22's flow owns it).
- Invented: the `settings` field on the `info` IPC reply; `rcli agent password set` reading stdin when
  `--password` is omitted.

## Comments

- Completed on branch `t/14` on top of the three `feat(gui)` commits above (`096fd47`, `0c2e8aa`): every item
  that section lists as "Not implemented yet" now exists, except the app-menu entry (installers 17/22/24 own
  it) and Windows UAC elevation (ticket 22's flow, as noted above).
- **Device Settings and unlock password editors.** The window edits Terminal User (text), root level and accept
  mode (selectable values) with one Apply button, and sets/clears the unlock password. Every change follows the
  same path as Reveal: the IPC first, and when the Agent refuses only for lack of root the same `rcli agent …`
  command runs elevated (`rcli agent unlock-password set` reads stdin, like `password set`, so no secret reaches
  a command line). Changes apply live (the Agent saves Device Settings at once).
- **Server Key and unlock password reveal.** Root-only fields `server_key` and `unlock_password` were added to
  the `info` reply (root callers only), so the elevated helper is the existing `rcli agent info --json`. The
  window shows them after one "Reveal Server Key and unlock password" (OS elevation) click.
- **Prompts (tickets 07/08) in the window.** `Platform::ask_root`/`ask_accept` are routed through the new
  `ipc::Prompts` queue by `gui::WindowPlatform`, a Platform wrapper `rcli agent run` installs on desktop builds:
  the prompt reaches the window over the IPC (`prompts` lists it), the person answers Accept/Reject in the
  window, and no answer in time auto-rejects (the 30 s rule). Accept is root-only (`prompt.answer` refuses it
  without root, so a local process cannot grant root or let a Controller in); Reject is open to any local user,
  like `disconnect`. The GUI's Accept runs `sudo rcli agent prompt answer <id> accept` elevated;
  `rcli agent prompts` lists the waiting prompts for scripting.
- **"Grant root to this session"** is a button per connected Controller (root, like ticket 07's `root.grant`).
- **The service opens the window** when someone starts controlling the Device — `WindowPlatform` opens it on
  the control indicator (Control Lock held / live stream), input, exec and shell — and for every prompt. It
  spawns `rcli` (no arguments) with the socket in `RCLI_AGENT_SOCKET`, reopening if the person closed it.
- **macOS permission guide**: the window lists the Screen Recording and Accessibility grants.
- Tests: `crates/agent/src/ipc.rs` (prompt queue listing/answer/timeout; the accept/reject permission split;
  root-only `info` secrets) and `crates/cli/src/gui.rs` (prompts reach the queue, not the platform dialog;
  timeout rejects). `cargo test --workspace` and clippy (with and without `--features gui`) are clean.
- Not verified here: anything visual or interactive — the window itself, polkit/macOS authorization dialogs,
  clipboard, and the manual checklist above (no display session was used; compile-checked only).
- Invented (on top of the two items above): `prompts`/`prompt.answer` and the accept/reject permission split;
  root-only `server_key`/`unlock_password` fields on `info`; the `WindowPlatform` decorator and its spawn;
  `rcli agent prompts` and `rcli agent prompt answer` as the GUI's (and scripts') entry points.
- shortcut: with a desktop build but no window answering (spawn failed, no display), a prompt waits its full
  timeout and then rejects instead of falling back to the platform's own dialog; upgrade when an OS adapter
  (tickets 28-35) supplies one.
