# What a Controller may do on a Device is set on the Device, not by Server roles

Like TeamViewer and RustDesk, the person who owns a Device decides what remote users may do on it. Device Settings live on the Device and apply equally to every Controller, whether it connects by Device Password or through the Dashboard. They set the Terminal User (by default the account that installed the Agent), whether root/SYSTEM is never allowed, asked for at the Device, or always allowed, and whether a person at the Device must accept each connection. Admin and Operator now matter only for administering the Server (Users, Server Key, Server settings, rollouts, Device IDs and removal, audit). This supersedes the "admin-only `--as root`" rule in ADR 0001.

## Consequences

- Unlock passwords move from the Server's Secrets Vault to the Device. They are stored encrypted in the OS secret store, as the Device Password is, and are set only at the Device. A Server breach no longer exposes Device login passwords, but nobody can set an unlock password remotely.
- On Linux and macOS, `sudo` inside the terminal works as it does over SSH. `--as root` (the only route on Windows) follows the Device's root setting, and the person at the Device can grant root to the current session from the GUI.
- A person at the Device can see who is controlling it and disconnect them without being root.

- Superseded by ADR 0005: rcli no longer stores unlock passwords at all.
