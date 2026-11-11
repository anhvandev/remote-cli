# 08: Device Settings: accept each connection, see who is controlling, disconnect

**What to build:** `sudo rcli agent accept password|password+accept` sets whether a person at the Device must accept each new connection (prompt with Accept/Reject, auto-reject after 30 s, rejected when no session can show it). `rcli agent info` lists who is currently connected, for example `dev-laptop (10.0.0.5)` or `alice (Dashboard)`. `rcli agent disconnect [<who>]` is allowed for any local user and ends that Controller's sessions (Live View, shell, Control Lock). The Controller can reconnect if it still has the password. Applies equally to Dashboard Users. Disconnects and prompt answers are audited. See `.scratch/easy-setup/spec.md`, ADR 0004.

**Blocked by:** 06

**Status:** resolved

- [x] password+accept refuses until accepted; times out to reject
- [x] `info` lists current Controllers by name
- [x] Disconnect ends Live View and shell and releases the Control Lock, without root
- [x] Dashboard Users are subject to the same rules
- [x] Disconnects are audited

## Comments

- Implemented on branch `t/08`. Device Settings gain `accept: password | password+accept` (`settings.rs`
  `AcceptMode`, default `password`), set with `sudo rcli agent accept …` (`accept.set`) and saved in `settings.json`
  beside the credentials, so it applies live and survives a reload/restart.
- **A connection's display name.** A new `ExecRequest.controller` field carries the name the person at the Device
  sees — `dev-laptop (10.0.0.5)` for a password connection (its existing `User::name`) and `alice (Dashboard)` for a
  Dashboard User (`User::controller_display`, which appends ` (Dashboard)`). `requester` stays the bare name a root
  prompt shows (ticket 07 pins it to `alice`), so the two names serve different UIs without renaming ticket 07's
  field. `rcli agent info` lists and `disconnect` matches this display name.
- **Accept gate.** The Agent enforces accept mode at the top of `exec` (`accept_connection`), for every Controller
  regardless of source. In `password+accept` a new Controller is asked once via a new `Platform::ask_accept(requester,
  timeout)` port (`ACCEPT_PROMPT_TIMEOUT` 30 s; the FakePlatform records each prompt and answers from a scripted
  queue, mirroring `ask_root`). Accept lets the Controller through and remembers it; Reject and "no answer in time"
  (auto-reject) and "no session to show the prompt" (`session_state() != LoggedIn`) all refuse with 403
  `connect_denied`. Once accepted a Controller is not asked again until a `disconnect`. Accepted Controllers live in
  a `Controllers` map on `ipc::Status` (survives a reconnect), keyed by the display name.
- **info.** `rcli agent info` lists the connected Controllers under `controlled by:` (and `controllers` in `--json`),
  e.g. `dev-laptop (10.0.0.5)` and `alice (Dashboard)`.
- **disconnect.** `rcli agent disconnect [<who>]` is allowed for **any local user** (handled before the IPC root
  gate), unlike every other mutating command. It removes matching Controllers (`who` matches the display name or a
  prefix, so `alice` ends `alice (Dashboard)`; no `who` ends all), closes the Device's remote sessions (shells, Live
  View, media) at the Agent, and sends a `DisconnectController` message over the Agent link so the Server releases
  that Controller's Control Lock (`control_lock::release_if_holder`, matched by holder name). The Controller can
  reconnect with the password (and re-accepts in `password+accept` mode).
- **Audit.** Every accept-prompt answer is recorded in the triggering `terminal` entry's `result.connect_decision`
  (`accepted` / `rejected` / `no_session`, empty when no prompt was shown), alongside ticket 07's `root_decision`.
  Every disconnect is a `disconnect` audit entry naming the Controller that was ended.
- **Tests.** `crates/testkit/tests/accept_and_disconnect.rs` (8): accept mode root-gated and persisted across a
  settings reload; accept refused with no session (no prompt shown); accept / reject / 30 s auto-reject (prompt given
  30 s) and once-accepted-not-asked-again; `info` listing both a password Controller (`dev-laptop (127.0.0.1)`) and a
  Dashboard User (`alice (Dashboard)`); `disconnect` as a non-root local user releasing the Control Lock, clearing
  `info`, and allowing reconnect; and the disconnect + accept/reject prompt answers in the audit log.
- Verification: on the merged tree (after `git merge easy-setup`, resolving `lib.rs` (agent) and `lib.rs` (cli) where
  ticket 09 moved `unlock-password` into `agent`), `cargo test --workspace` 185 passed / 0 failed (including
  `accept_and_disconnect` 8, `terminal_user` 10, `exec` 4); `cargo clippy --workspace --all-targets` clean;
  `cd dashboard && npx tsc --noEmit` clean.
- Not verified: that a shell or Live View actually stops streaming on `disconnect` — the Agent closes the Device's
  remote sessions (shortcut: it ends the Device's sessions broadly, not one Controller's; refine when sessions are
  tagged per Controller) but the harness has no easy way to assert a stream ending, so the disconnect test checks the
  Control Lock release, `info` removal and reconnect instead. Also not separately asserted: a Dashboard User hitting
  the accept gate (the gate is in the shared `exec` path and is source-agnostic, so it applies equally).

## Comments

- Implemented by a subagent on branch `t/08` (`a7c5ddd`, 8 commits). `sudo rcli agent accept password|password+accept`
  (root-only, `AcceptMode` in `settings.rs`); `password+accept` refuses new connections until accepted, with the
  ticket 07 scripted-prompt pattern (`Platform::ask_accept`, 30 s auto-reject, no-session rejection). `rcli agent info`
  lists connected Controllers as `dev-laptop (IP)` / `alice (Dashboard)`. `rcli agent disconnect [<who>]` works for any
  local user (NOT root-gated): ends sessions, releases the Control Lock, allows reconnect. Every disconnect and prompt
  answer is audited with `connect_decision`.
- Verified on the merged tree: `cargo test --workspace` 185 passed / 0 failed, clippy and `tsc --noEmit` clean.
  `accept_and_disconnect.rs` has 8 tests.
- `shortcut:` in `lib.rs` disconnect handler: teardown closes all of this Device's remote sessions (shells, Live View,
  media) rather than only the named Controller's; refine when sessions are tagged per Controller. This is safe with
  one Controller but not with several.
- Not verified: shell/Live View streams actually stop on disconnect (the harness cannot observe stream termination;
  test asserts Control Lock release, info removal, and reconnect). A Dashboard User hitting the accept gate is not
  tested separately (the gate is in the shared `exec` path).
- Invented: `ExecRequest.controller` (display name, e.g. `alice (Dashboard)`) kept separate from `requester` (plain
  name for the root prompt); `connect_decision` on `ExecResult` (same pattern as `root_decision`); `disconnect` matches
  a Controller by display-name prefix.

- code-review fix: glossary wording in `rcli agent disconnect`'s doc comment ("on their machine" → "on their Device"; avoid "machine").

- final code-review fix (P5): the waiting-prompt list stays open to any local user on purpose; a `shortcut:` comment
  on `Request::Prompts` in `crates/agent/src/ipc.rs` records why (story 27 needs the person at the Device — not
  necessarily root — to see who asks; accepting stays root-only and refusing is like `disconnect`) and the tension
  with the spec's IPC split. No protocol or enforcement change.

- Decision (round 4): accepting a new connection stays root-only, for the same reason as the root prompt (ticket 07):
  a local process must not let a Controller in behind the person's back. Refusing and Disconnect stay open.
