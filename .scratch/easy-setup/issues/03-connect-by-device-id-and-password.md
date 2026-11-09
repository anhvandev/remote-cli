# 03: Connect with Device ID + Device Password from the CLI

**What to build:** Tracer bullet for the TeamViewer-style flow. At registration the Agent generates a Device Password (10 characters, no look-alikes) and keeps it in a Secret Store (this ticket: the encrypted-file fallback with a root-only key; OS stores come with the platform tickets). The Server keeps only a hash. The running Agent opens a local IPC socket. `rcli agent info` shows the Device ID and connection status to any local user, and `sudo rcli agent password show` shows the password to root only. On the Controller side, `rcli setup` saves the Server Address and Server Key (and pins the update key) in the per-user config. `rcli connect <Device ID>` asks for the Device Password once and remembers it in the OS keychain (file fallback). After that every Device command takes just the Device ID, and `rcli forget <Device ID>` removes the password. A password connection reaches only its own Device and no admin endpoint. It is named `<controller hostname> (<IP>)` in audit entries and as Control Lock holder. Server roles stop gating anything that runs on a Device (Device Settings take over in ticket 07). See `.scratch/easy-setup/spec.md`, ADR 0003/0004.

**Blocked by:** 02

**Status:** ready-for-agent

- [x] Fresh Controller: `rcli setup`, `rcli connect <ID>`, then `rcli shot <ID>` works with no env vars
- [x] A wrong password at `connect` is reported and not saved
- [x] A password connection gets an error for any other Device and for admin endpoints
- [x] Audit and Control Lock holder show `<hostname> (<IP>)`
- [x] `rcli agent info` works for any local user; `password show` is refused to non-root
- [x] The password is never stored in plaintext on the Device, the Server or the Controller config file
- [x] CLI tests use a per-test config directory and a file-backed keychain

## Comments

- Implemented on branch `t/03`. Vertical slices, each with its test first:
  - **Device Password** (`rcli_protocol::password`): 10 chars from an alphabet without `0/O`/`1/l/I`, `generate()`,
    argon2id `hash()`/`verify()`, `MIN_LENGTH = 8` for the change path (ticket 06). Registration (`registration.rs`,
    migration `03002`) generates it, returns it in `RegisterResponse.device_password` and stores only the hash in
    `devices.device_password_hash`. `Credentials.device_password` is `skip_serializing`, so it never reaches
    `credentials.json`.
  - **Secret Store port** (`rcli_agent::secret_store`): a two-method trait (`get`/`set`) plus `FileSecretStore`, the
    root-only-key fallback: a random 32-byte `.secrets.key` (0600) seals each secret in its own file with
    AES-256-GCM and the secret's name as AAD. Tickets 16/22/24 add implementations without touching callers.
  - **Local IPC** (`rcli_agent::ipc`): a `tokio` Unix socket, one newline-delimited JSON request per connection,
    only `tokio::io` traits so a named pipe can reuse `handle`. `rcli agent info` (any local user) and
    `rcli agent password show` (refused unless `SO_PEERCRED`/`getpeereid` says uid 0) are the only commands; the
    socket defaults to `/run/rcli/agent.sock` (`/var/run/rcli/agent.sock` on macOS) and is overridable with
    `--socket`/`RCLI_AGENT_SOCKET`. `rcli agent run` starts it; `TestAgent` carries its `Status` so tests drive the
    socket from a temp dir.
  - **Second auth principal** (`auth::authenticate_controller`, `api::Caller`): either `Authorization: Bearer` or
    `X-Rcli-Server-Key` + `X-Rcli-Device` + `X-Rcli-Device-Password` + `X-Rcli-Controller`. The principal is scoped to
    its own Device (`resolve_for`) and is never an admin, so `/users`, `/server-key` and rollouts answer
    `admin_required`; `GET /devices` is filtered to that Device. Its name is `<hostname> (<IP>)` (peer IP from
    `ConnectInfo`), used in audit entries and as Control Lock holder.
  - **Controller config** (`rcli::config`): `<config dir>/config.json` holds the Server Address, Server Key and the
    update key pinned in `setup` (over the channel the Server Key's fingerprint verifies: the CA from
    `/api/v1/server-ca` is checked against the key before `/api/v1/update-key` is fetched). Remembered Device
    Passwords are `0600` files under `<config dir>/passwords/`. `RCLI_SERVER`/`RCLI_TOKEN` are gone; `--server` and
    `--token` still exist for admin scripting and tests.
  - **Roles**: `exec`/`shell --as root` and the `no_logged_in_user` fallback no longer check `is_admin()`, and
    restart/shutdown/sleep no longer let an admin bypass the Control Lock (ADR 0004). Server administration keeps
    `Admin`. The three tests that asserted the old rules were rewritten to the new ones.
- Verification: on the merge of `easy-setup`, `cargo test --workspace` 139 passed / 0 failed;
  `cargo clippy --workspace --all-targets` and `cd dashboard && npx tsc --noEmit` clean. The dashboard is untouched
  by this ticket, so Playwright was not run here (ticket 10's Server Settings page merged in cleanly and its
  Playwright spec is ticket 10's to run).
- Not verified: the OS keychain (only the `0600` file fallback exists, as the ticket allows) and a real named pipe
  (Windows) — both are `shortcut:` comments. `rcli connect` without `--password` reads a prompt with `rpassword`;
  tests pass `--password`.
- Invented where the spec is silent: the `X-Rcli-*` header names; the controller's stable audit/Control Lock id
  (first 16 bytes of SHA-256 of the display name, so it survives reconnects); the public `GET /api/v1/update-key`
  endpoint used to pin the key at setup; and `forbidden_device` (403) for another Device's endpoints versus
  `admin_required` (403) for Server administration.
