# Spec: Easy setup: one CLI, Server Key, Device ID + Device Password

Status: ready-for-agent

Vocabulary follows `GLOSSARY.md` (Device, Device ID, Device Password, Device Settings, Terminal User, Server Key, Agent, Server, Controller, Control Lock, Live View, Operator, Admin). Decisions follow `docs/adr/0003` (Server Key and Device ID + Device Password replace Enrollment Codes and API tokens) and `docs/adr/0004` (Device Settings, not Server roles, govern what a Controller may do on a Device; supersedes the admin-only `--as root` rule in ADR 0001). This spec supersedes these parts of `.scratch/remote-cli/spec.md`: Enrollment & Credentials, Secrets Vault, API tokens, the admin-only `--as root` rule, and the installer parts of the platform tickets 29, 31 and 33. Everything else there still holds.

## Problem Statement

Getting a machine under management, or starting to control one, takes too many steps. An admin must create an Enrollment Code for every Device. The Agent needs a CA file, a CA fingerprint, an update key, a state directory and a name passed as flags or env vars. Every Controller needs a hand-made API token plus `RCLI_SERVER` and `RCLI_TOKEN` in its environment. There is no installer, no background service, and no page telling people how to connect. Unlike TeamViewer or RustDesk, the person who owns a Device has no say in what remote users may do on it.

## Solution

- **One binary, `rcli`,** acts as both Controller and Agent. The Server stays separate, with its env and Docker Compose unchanged except `RCLI_UPDATE_PUBLIC_KEY`, which it now hands to Devices at registration.
- **Install once, then forget.** An install needs only the Server Address and the Server Key. It can be a one-line `curl … | sudo sh` or PowerShell command, a self-installing exe, a `.dmg`, a deb/rpm, or a headless `rcli agent install`. The Device registers itself, gets a 9-digit Device ID and a static Device Password, and the Agent runs as a background service.
- **Connect like TeamViewer.** A Controller runs `rcli setup` once (Server Address + Server Key), then `rcli connect <Device ID>` with the Device Password. No User account and no token. Admin-only commands use `rcli login` with the Dashboard account.
- **The Device decides.** Device Settings on the Device govern every Controller: Terminal User, root access (never / ask / always), whether a person must accept each connection, and a Disconnect button. A small GUI window and `rcli agent …` commands show and change all of it live through the running service.
- **The Dashboard helps.** A Connect guide page has the key, the copyable install commands, downloads, Controller setup and an MCP snippet. A Server Settings page lets an Admin view and replace the Server Key and set the Server Address.

## User Stories

### Install and register

1. As a person setting up a machine, I want to install the Agent with only the Server Address and Server Key, so that onboarding takes one step.
2. As a person setting up a Linux or macOS machine, I want to paste one `curl … | sudo sh` command copied from the Dashboard, so that I don't have to download anything by hand.
3. As a person setting up a Windows machine, I want either a PowerShell one-liner or a self-installing `rcli.exe`, so that I can install it without reading docs.
4. As a person setting up a Mac, I want a `.dmg` with an app that installs itself and guides me through the Screen Recording and Accessibility grants, so that macOS permissions don't block me.
5. As a person setting up a headless server, I want `sudo rcli agent install --server … --key …`, so that I can install without a display.
6. As a person setting up a machine, I want the Agent to run as a background service right after install and after every reboot, so that I never start it by hand.
7. As a person who installed the Agent, I want the Device to register itself and appear in the fleet without an admin approving it, so that nobody has to create codes.
8. As a person who installed the Agent, I want it to verify that it is talking to the real Server using the fingerprint inside the Server Key, so that nobody can impersonate the Server.
9. As a person who installed the Agent, I want it to pin the update-signing key when it registers, so that a later Server breach cannot push updates signed with another key.
10. As a person removing the Agent, I want `sudo rcli agent uninstall` to stop the service, remove the Device from the Server and delete local data, so that nothing is left behind.

### Device ID and Device Password

11. As a person at a Device, I want to see its 9-digit Device ID at any time, so that I can give it to whoever will connect.
12. As a person at a Device with root/Administrator rights, I want to see and copy its Device Password, so that I can hand it over.
13. As a person at a Device with root/Administrator rights, I want to change the Device Password, so that I can cut off people who knew the old one.
14. As a person at a Device, I want the Device Password stored encrypted, so that a copied disk or backup does not reveal it.
15. As a person at a Device, I want to change the Device ID and the Device name from the GUI or the CLI, so that I can keep a memorable ID or keep the old ID after a reinstall.
16. As an Admin, I want to change a Device's ID or name and set a new Device Password from the Dashboard, so that I can manage headless machines remotely. I never see the current password.

### Controller

17. As a Controller, I want `rcli setup` to ask for the Server Address and Server Key once and save them, so that I don't need env vars.
18. As a Controller, I want `rcli connect <Device ID>` to ask for the Device Password once and remember it in my OS keychain, so that later commands just take the Device ID.
19. As a Controller, I want `rcli forget <Device ID>` to remove a remembered password.
20. As a Controller using a password connection, I want every Device command (devices, shot, act, batch, exec, shell, power, wake, lock, unlock, metrics, displays) to work as before, so that nothing is lost.
21. As an AI agent, I want `rcli mcp` to use the same saved configuration and passwords, so that the MCP setup is one line.
22. As an Admin using the CLI, I want `rcli login` with my Dashboard account (password + TOTP), so that admin commands work without hand-made tokens.
23. As a Controller, I want `rcli root on <Device ID>` so that every exec and shell to that Device asks for root without a flag each time. The Device still decides.
24. As a Controller, I want `rcli upgrade` to update my `rcli` to the build the Server is rolling out, verifying its signature, so that I stay current easily.

### Device Settings

25. As the owner of a Device, I want terminal commands and shells to run as the account that installed the Agent by default, and to be able to change that account, so that remote users get the rights I choose.
26. As the owner of a Device, I want to choose whether root/SYSTEM is never allowed, asked for at the Device, or always allowed, so that I control elevation. `sudo` inside the terminal keeps working as it does over SSH.
27. As a person at a Device set to "ask", I want a prompt naming who asks for root, with Accept/Reject and auto-reject after 30 seconds, so that nobody gets root without me.
28. As a person at a Device, I want a button that grants root to the current session only, so that I can help without changing the setting.
29. As the owner of a Device, I want to require a person at the Device to accept each connection in addition to the password, so that a leaked password alone is not enough.
30. As a person at a Device, I want to see who is controlling it (for example "dev-laptop (10.0.0.5)" or "alice (Dashboard)") and disconnect them, even without root, so that I can always stop a session on my machine.
31. As the owner of a Device, I want the unlock password stored on the Device, encrypted, and set only at the Device, so that a Server breach never exposes login passwords.
32. As an owner, I want every change made in the GUI or with `rcli agent …` to apply to the running service at once, without a restart.
33. As a Dashboard User, I want the same Device Settings to apply to me as to password connections, so that the owner's rules hold for everyone.

### Security

34. As an owner, I want the Server to lock out repeated wrong passwords per Device ID and IP (5 failures → 1 minute, doubling, at most 1 hour) and audit every failure, so that a static password cannot be brute-forced.
35. As an Admin, I want to replace the Server Key's secret without breaking registered Devices, so that a leaked key only needs new installs and Controllers to update.
36. As an Admin, I want audit entries and the Control Lock holder to name password connections by Controller machine name and IP, so that I can still trace who did what.

### Dashboard

37. As an Admin, I want a Server Settings page to view and replace the Server Key and set the Server Address, so that I don't edit env for those.
38. As a User, I want a Connect guide page with the Server Address, the Server Key, copyable install commands per OS, downloads, Controller setup, an MCP snippet, and notes on bypassing SmartScreen and Gatekeeper for unsigned builds, so that anyone can onboard a machine.
39. As a User, I want a red warning on the guide page when the Dashboard is served over http, so that I know the one-line install can be tampered with.
40. As a User, I want Devices without a display to hide the Screen tab and say why, so that I don't try to view a screen that doesn't exist.

### Headless Devices

41. As a Controller, I want a Device without a display to keep metrics, exec, shell, power and Wake, and to refuse shot, act and Live View with a clear "Device has no display" error.

## Implementation Decisions

- **One binary.** The Agent's `main` becomes `rcli agent run`, inside the CLI crate. The Agent stays a library crate. `rcli-agent` is removed. The testkit starts Agents the same way.
- **Server Key** = `rk_<CA fingerprint>_<secret>`. The Server generates it on first start and keeps it in Postgres. Replacing it changes only the secret. The CA never changes. Both the Agent's registration and every password connection present the secret; a User session does not (any signed-in User can read the key, see ADR 0003). The fingerprint pins the CA on the Agent port, replacing `--ca` and `--ca-fingerprint`; the Controller API sits behind a TLS reverse proxy, so `rcli setup` requires an `https://` address instead.
- **Registration** replaces enrollment: Server Key → per-Device mTLS certificate + a 9-digit Device ID (random and unique; changeable only on purpose) + the pinned update key. Enrollment Codes are removed from the Server, CLI, Dashboard and DB.
- **Device Password:** 10 characters from an alphabet without look-alikes (no `0/O`, `1/l/I`), generated at registration, at least 8 characters when changed. The Agent keeps it in a **Secret Store** port: DPAPI machine scope on Windows, the System Keychain on macOS, `systemd-creds` (TPM when present) on Linux, and a fallback encrypted with a root-only key file. The Server keeps only a hash (argon2). Existing sessions survive a change.
- **Controller authentication:** two kinds of principal on the Controller API. (1) A *password connection*: Server Key secret + Device ID + Device Password. It reaches only that Device and has no Server-admin rights. Its display name is `<controller hostname> (<IP>)`. (2) A *User session*: Dashboard sign-in, or `rcli login` storing a session in the CLI config. API tokens and `rcli token` are removed. Rate limiting is per (Device ID, IP) as in story 34.
- **Server roles:** Admin and Operator only gate Server administration (Users, Server Key, Server Settings, rollouts, Device ID / name / password reset, Device removal, audit). Nothing about what runs on a Device is checked against roles.
- **Device Settings** live in the Agent's local config and are enforced by the Agent: Terminal User (default: the installing account, `SUDO_USER` or the Windows user running the installer), root `never | ask | always` (default `ask`; `--allow-root` at install sets `always`), accept mode `password | password+accept`, and session root grants. Prompts appear through the GUI in the active session. With no session to show them in, they are rejected.
- **Local IPC:** the service listens on a Unix socket (Linux, macOS) or a named pipe (Windows). `rcli agent info | password show|set | id set | name set | server set | terminal-user set | root never|ask|always | accept … | unlock-password set|clear | disconnect` and the GUI are clients. Any local user may read the ID, the connection status and who is controlling, and may disconnect. Everything else requires root/Administrator. Changes apply live. ID, name and password-hash changes are sent to the Server (the ID is checked for uniqueness).
- **Unlock passwords** move to the Agent's Secret Store. The Server's Secrets Vault, its API, `rcli unlock-password` and the Dashboard UI are removed. `RCLI_MASTER_KEY` remains for the CA key and TOTP secrets.
- **CLI config:** a config file in the per-user config directory (the platform's standard location) holds the Server Address, Server Key, pinned update key and User session. Remembered Device Passwords live in the OS keychain (Keychain, Credential Manager, Secret Service), with a `0600` file fallback. `RCLI_SERVER` and `RCLI_TOKEN` are removed. `rcli update …` is renamed `rcli rollout …`. `rcli upgrade` replaces the running binary (and the service binary when run as root).
- **Server Settings:** the Server Address (replacing `RCLI_AGENT_HOSTNAMES`) and the Server Key are stored in Postgres and edited by Admins. Changing the address reissues the Server's TLS certificate. All other Server config stays in env.
- **Downloads and install scripts:** the Server serves `install.sh`, `install.ps1` and `/download/<os>/<arch>` from the build currently rolled out (the signed builds from ticket 17 of remote-cli). The scripts download `rcli`, then run `rcli agent install --server … --key …`.
- **GUI:** one small native window (egui), launched from the app menu or by running `rcli` with no arguments on a desktop. It is an IPC client: ID, password (reveal/copy/change after an OS elevation prompt: polkit, UAC or macOS authorization), Server Address/Key, Device Settings, unlock password, "being controlled by …" with Disconnect and grant root, and on macOS the permission guide. No tray icon. The service opens it when someone starts controlling the Device and for prompts. The binary must still start on machines without display libraries.
- **No migration.** Old enrollment, token and vault data is dropped. A new Server can start from an empty DB.

## Testing Decisions

- **What a good test is:** it drives the system only from the outside (the Controller API, or the `rcli` command line) and asserts on what comes back: responses, typed errors, Observations, audit entries, Device Status. It never reaches into module internals, so it survives refactors behind the seam.
- **One primary seam: the in-process test system.** As today, each test runs a real Server on Postgres in a test container plus one or more Agents on the FakePlatform, controlled by the injectable clock. Tests call the Controller API or run `rcli` in-process. The helpers change: "create an Enrollment Code and enroll" becomes "read the Server Key and register", and "run rcli with a token" becomes "run rcli with its own per-test config directory and a file-backed keychain".
- **Agent local IPC through the same seam.** Device Settings are changed on the Device, not through the Controller API, so each test Agent opens its IPC socket in a temp directory and tests drive it **with `rcli agent …` in-process**, never with raw socket messages. Prompts (root "ask", "password + accept") are answered by scripted replies on the FakePlatform.
- **What this seam covers:**
  - registration with a right or wrong Server Key secret and fingerprint; key replacement leaving registered Devices Online; update-key pinning
  - password connections reaching only their own Device and no admin endpoint; rate-limit lockout and doubling; failures audited; holder and audit naming `<hostname> (<IP>)`
  - Device ID uniqueness on change from the Device and by an Admin
  - Device Settings: Terminal User, root never/ask/always (including timeout and no-session rejection), session root grant, accept mode, disconnect; all applying equally to Dashboard Users; `rcli root on`
  - the IPC permission split (non-root may read ID/status and disconnect, nothing else) and live application of changes
  - `unlock` using the Agent-side password, never leaking it, masked in audit
  - "Device has no display" errors
  - CLI: `setup`, `connect`, `forget`, `login`, `rollout`, `upgrade` (accepts good signatures, rejects bad ones); MCP from saved config
- **Prior art:** the existing in-process suites for enrollment, auth, the vault, the CLI and MCP are the ones to replace or extend; the other suites (control lock, exec, shell, power, update) show the style.
- **Dashboard:** Playwright against the dev stack with the FakePlatform, in the style of the existing admin tests: Server Settings page (replace key, change address, Admin Device actions) and the Connect guide page (blocks present, copy buttons, http warning).
- **Real machines:** the existing Platform contract suite for each adapter, plus a manual checklist for install, reboot, uninstall and packages, run by the user. The agent writes the code, cross-builds, and supplies the exact commands.
- **GUI:** manual checklist only.

## Out of Scope

- Code-signing and notarization (builds stay unsigned for now; the guide page explains the warnings).
- A tray icon, and auto-update for Controller-only machines (they use `rcli upgrade`).
- A Device Setting to refuse Dashboard Users without the password.
- Migrating existing Devices, tokens or unlock passwords.
- Per-Device permissions on the Server.

## Further Notes

- Linux first (X11, then Wayland), then Windows, then macOS.
- Wayland lock screen and greeter still depend on the user running the Wayland lockscreen probe. The fallback is to report "unavailable", as the macOS login window does.
- The Server Key's secret is a weak gate (see ADR 0003). The Device Password plus rate limiting is the real protection, so rate limiting must not be cut.
