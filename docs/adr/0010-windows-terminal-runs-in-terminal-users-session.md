# On Windows, terminal commands run in the Terminal User's signed-in session, or not at all

The Windows Agent runs as a SYSTEM service. Until now, `exec` and `shell` on Windows ran every command as SYSTEM in session 0, whatever the Device Settings said. That was a shortcut left over from when the Agent ran inside the user's session, and it meant "root never" had no effect and audit recorded `as_root: false` for SYSTEM commands. We now run a non-root terminal command with the token of a session the Terminal User is signed in to (console or RDP), on that session's desktop and with that user's environment. When the Terminal User has no signed-in session, for example at the login screen, the command is refused with `terminal_user_not_signed_in`. Root commands (`--as root`) still follow the Device's root setting and run as SYSTEM.

## Considered Options

- **S4U logon** (a token for the Terminal User without a password): it would work when nobody is signed in. Rejected: we have not verified it for local accounts, the token has no network credentials, and it would be a second path to maintain.
- **Fall back to SYSTEM when the Terminal User is not signed in:** this keeps the old behaviour. Rejected: it is the bug, and it silently breaks the "root never" promise.
- **`LogonUser` with the user's password:** rejected by ADR 0005, because rcli stores no OS password.

## Consequences

- Behaviour change: a non-root `exec` or `shell` on Windows at the login screen used to succeed as SYSTEM and now fails. Use `--as root`, which follows the root setting.
- Commands run where the user's GUI is, so GUI command-line tools started through `exec` can see that user's desktop.
- `ExecResult.as_root` and the audit entry report who the command really ran as.
- Builds on ADR 0004, which says the Terminal User comes from Device Settings, and on ADR 0005.
