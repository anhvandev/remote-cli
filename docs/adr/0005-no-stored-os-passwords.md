# rcli never stores a Device's OS login password; the Controller types it

Controlling a Device at its lock or login screen needs the OS account's password. Until now an Admin, and later the person at the Device, could store it as an "unlock password" (first on the Server, then in the Device's Secret Store), and `rcli unlock` had the Agent type it. We drop this. rcli keeps no OS password anywhere. The privileged Agent (ADR 0001) only makes the lock and login screens visible and controllable. A Controller that wants to get past them types the password itself with a `type` Action marked `secret`, and audit masks it as it already masks anything typed at a lock or login screen. Where the Controller gets the password (a person, a password manager, an AI agent's prompt) is outside rcli.

## Considered Options

- Keep the unlock password in the Device's Secret Store (ADR 0004): one command unlocks, and an AI agent never sees the password. Rejected: the product promise is that it holds no credential of the Device's accounts, and a stored password is one more secret to protect on every Device.
- Keep it, but off by default: same code to maintain, and the promise holds only per Device. Rejected.

## Consequences

- Supersedes the unlock-password sentences in ADR 0001 and the first Consequence of ADR 0004.
- `rcli unlock`, the `unlock` endpoint, `rcli agent unlock-password` and the Agent GUI's unlock section are removed with no compatibility shim. The protocol field is reserved so mixed versions keep working during a rollout.
- An upgraded Agent deletes any unlock password left in its Secret Store.
- An AI agent must be given the OS password to get past a lock screen, so it sees it. The macOS login window stays view-only (ADR 0001), so Macs still need auto-login after a reboot.
