# 07: Device Settings: Terminal User and root access

**What to build:** exec and shell run as the Device's Terminal User: by default the installing account (for now, the account running the Agent), changeable with `sudo rcli agent terminal-user set`. `sudo` inside the terminal works as it does over SSH. Root access is set with `sudo rcli agent root never|ask|always` (default `ask`). `--as root` on exec/shell asks the Device. `ask` raises a prompt naming the requester, with Accept/Reject and auto-reject after 30 s, and with no session to show it in, it is rejected. The person at the Device can grant root to the current session only. On the Controller, `rcli root on|off <ID>` makes `--as root` the default for that Device. Rules apply equally to password connections and Dashboard Users. Every root run and prompt answer is audited. The FakePlatform answers prompts from a script. See `.scratch/easy-setup/spec.md`, ADR 0004.

**Blocked by:** 06

**Status:** ready-for-agent

- [x] exec/shell run as the Terminal User
- [x] never / ask / always behave as specified, including timeout and no-session rejection
- [x] A session root grant ends with the session
- [x] Dashboard Users are subject to the same rules
- [x] `rcli root on <ID>` makes exec/shell request root by default
- [x] Root runs and prompt answers are audited

## Comments

- Implemented on branch `t/07`. Device Settings live in `crates/agent/src/settings.rs`, saved as `settings.json`
  beside `credentials.json` in the Agent's state directory (`Status::set_settings_dir`), and shared with the running
  Agent through `settings::Handle`, so `rcli agent …` changes apply at once and survive a restart.
- **Terminal User.** `sudo rcli agent terminal-user set <user>` (`terminal-user.set`) writes the account; the
  default is `SUDO_USER`, else the account running the Agent (`USER`/`USERNAME`). `RunAs` in `platform.rs` became
  `RunAs::User(String)` instead of `LoggedInUser`, and exec/PTY both take it, so a command runs as the Terminal User
  whichever console session is current. `sudo` inside the terminal keeps working: the FakePlatform's `fake_shell`
  now runs a `sudo …` line as root, and on a real adapter the Terminal User's own sudo rights apply as over SSH.
- **Root.** `sudo rcli agent root never|ask|always` (default `ask`) sets `root.set`. `run_as` maps `--as root` to
  `always` (allow), `never` (403 `root_denied`), and `ask` (prompt). The Agent asks through a new
  `Platform::ask_root(requester, timeout)` port with a 30 s timeout (`ROOT_PROMPT_TIMEOUT`); the FakePlatform
  records every prompt and answers from a scripted queue. A refusal (Reject or timeout) and "no session to show the
  prompt in" both surface as 403 `root_denied`; the no-session case is decided by `session_state() != LoggedIn`
  (nobody at the console = nobody to ask), because the Device, not the Server, owns the prompt.
- **Requester name.** The Server sends `ExecRequest.requester`/`ShellOpen.requester` = `User::name`, i.e.
  `alice` for a Dashboard User and `<controller host> (<IP>)` for a password connection (the existing controller
  user name), so the prompt and the audit entry name the actual principal. Server roles still gate nothing here.
- **Session grant.** `rcli agent root grant <who>` (person at the Device / GUI later) sets a `Grant` for the current
  session; `run_as` returns root with decision `session` only for that requester, and `run` clears the grant on
  every (re)connect, so it ends with the session.
- **Controller default.** `rcli root on|off <Device ID>` stores a per-Device flag in the Controller config
  (`config::root_default`/`set_root`) and `exec`/`shell` (and the MCP `exec` tool) send `as: root` when set and the
  caller did not pass `--as`. The Device still decides.
- **Audit.** Each `terminal` and `shell.start` audit entry now carries `result.root_decision` (`always`, `session`,
  `accepted`, `rejected`, `no_session`, `never`, or empty when root was not asked for), so both allowed and refused
  root runs and prompt answers are recorded. Root refusals are also audited (decision + `error`).
- **Tests.** `crates/testkit/tests/terminal_user.rs` (10): Terminal User default and change, `sudo` inside the
  terminal, persistence across a restart, non-root refusal, ask accept/reject/timeout-30s, no-session rejection,
  password-connection naming (`test-controller (…)`), `never`, session grant (and it ending across a new Agent
  session), `rcli root on|off`, and the audit entries. `exec.rs`/`shell.rs` updated where they encoded the old
  rules (run as the logged-in user, root unchecked by Device settings, login screen = root): the `--as root`
  success cases now set the Device to `always` through the trusting socket.
- Verification: on the merged tree (after `git merge easy-setup`), `cargo test --workspace` 176 passed / 0 failed;
  the affected suites re-run alone — `terminal_user` 10, `exec` 4, `shell` 6, `device_identity` 5.
  `cargo clippy --workspace --all-targets` clean; `cd dashboard && npx tsc --noEmit` clean. No dashboard change, so
  Playwright is unaffected (not run).
- Not verified: a real OS adapter running a command as another account (`RunAs::User` is honored by the
  FakePlatform; ticket 15+ adapters will need `su`/`runas`), and the GUI that shows prompts (ticket 14).
- Invented where the spec is silent: the `Platform::ask_root` port and its scripted FakePlatform answers; the
  `root_decision` field on `ExecResult`/`ShellOpened` and in audit results; `rcli agent root grant <who>` as the
  IPC entry point for "a person at the Device grants root to this session" (the GUI will call the same command);
  storing `rcli root on|off` as a per-Device file in the Controller config; the `Requester` plumbing over the proto.
