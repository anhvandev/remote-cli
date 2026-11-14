# 01: On Windows, terminal commands run as the Terminal User in their signed-in session

**What to build:**
- Fix the bug where `exec` and `shell` on Windows run every command as SYSTEM in session 0, ignoring Device Settings. Today "root never" has no effect on Windows, and `as_root` plus the audit entry report `false` for SYSTEM commands.
- Follow ADR 0010:
  - A non-root command runs with the token of a session the Terminal User is signed in to (console or RDP), on that session's desktop and with that user's environment, so GUI command-line tools see the user's desktop.
  - When the Terminal User has no signed-in session, the command is refused with `terminal_user_not_signed_in`.
  - `--as root` still follows the Device's root setting and runs as SYSTEM.
- The Agent already starts processes as the console user for the GUI. Extend that path with:
  - stdout/stderr pipes;
  - waiting for the exit code;
  - killing the process tree on timeout;
  - choosing the session by the Terminal User, not just the console.
- Update the outdated `shortcut:` comment.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `ExecResult.as_root` and the audit entry report who the command really ran as.
- [ ] Checklist on a real Windows Device, with results appended under `## Comments`:
  - [ ] with the Terminal User signed in, `rcli exec <dev> -- whoami` prints that user, not `nt authority\system`;
  - [ ] the same command at the login screen fails with `terminal_user_not_signed_in`;
  - [ ] with root set to "never", `--as root` is refused and a plain `exec` runs as the user;
  - [ ] with root allowed, `--as root` runs as SYSTEM and audit shows `as_root: true`;
  - [ ] `rcli shell <dev>` opens as the Terminal User;
  - [ ] a command that outlives its timeout is killed along with its children;
  - [ ] with the Terminal User signed in over RDP only, `exec` runs in that RDP session.
- [ ] `docs/usage.md` states the Windows rule and the behaviour change at the login screen.
