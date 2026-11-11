# 03: Connect with Device ID + Device Password from the CLI

**What to build:** Tracer bullet for the TeamViewer-style flow. At registration the Agent generates a Device Password (10 characters, no look-alikes) and keeps it in a Secret Store (this ticket: the encrypted-file fallback with a root-only key; OS stores come with the platform tickets). The Server keeps only a hash. The running Agent opens a local IPC socket. `rcli agent info` shows the Device ID and connection status to any local user, and `sudo rcli agent password show` shows the password to root only. On the Controller side, `rcli setup` saves the Server Address and Server Key (and pins the update key) in the per-user config. `rcli connect <Device ID>` asks for the Device Password once and remembers it in the OS keychain (file fallback). After that every Device command takes just the Device ID, and `rcli forget <Device ID>` removes the password. A password connection reaches only its own Device and no admin endpoint. It is named `<controller hostname> (<IP>)` in audit entries and as Control Lock holder. Server roles stop gating anything that runs on a Device (Device Settings take over in ticket 07). See `.scratch/easy-setup/spec.md`, ADR 0003/0004.

**Blocked by:** 02

**Status:** resolved

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
- Merged into `easy-setup` (fast-forward to `e6affcf`). Merger review found no code defect; one doc bug was fixed
  on `easy-setup` after the merge: `docs/deploy.md` still told admins to `export RCLI_TOKEN`, which no longer does
  anything, so the following `rcli user set-password` and `rcli server-key show` would fail. It now passes
  `--token` explicitly.
- Security review (merger):
  - Device scoping: every Device-work handler resolves through `resolve_for`, which refuses another Device with
    `forbidden_device`; `GET /devices` filters the list. Checked all routes: actions/exec/shell/live/history/power/
    wake/control-lock/processes/show use `resolve_for`; the remaining `resolve` callers (`vault.rs`, `audit.rs`,
    `updates.rs`, Device rename/delete/merge/revoke/force-release) are `Admin`-gated, so a password connection gets
    403 from the extractor before reaching them. Shell input/resize are limited to the caller's own session by
    `shell::Shells::device_of` (owner id), so a password connection cannot reach another Device's session.
  - Plaintext storage: Server keeps only `devices.device_password_hash` (argon2id; migration `03002`); there is no
    read-back endpoint. The Agent stores the password only via `FileSecretStore` (AES-256-GCM, 0600 key file);
    `Credentials.device_password` is `skip_serializing`, so `credentials.json` never holds it. The Controller writes
    the remembered password to a `0600` `passwords/<device>.json`; `config.json` holds only server/key/update key/token.
  - Admin gating: the dropped `is_admin` checks are exactly the Device-run ones — `exec --as root`,
    `shell --as root`, the `root_if_nobody_logged_in` fallback, and the admin Control Lock bypass for
    restart/shutdown/sleep. Every Server-admin endpoint keeps `Admin` (users, tokens, server-key, server-settings,
    rollouts/builds, Device rename/delete/merge/revoke, audit, unlock password, force-release). `is_admin` also
    now refuses a password connection even if its role string were spoofed (`device_scope.is_none()`).
  - Env vars: `RCLI_SERVER`/`RCLI_TOKEN` args are gone from `Cli`; `rcli mcp` builds its `Api` from the saved config
    (`api_target`) and auths per request, falling back from a User session to the saved Device Password.
- Verified on the merged tree (`e6affcf`): `cargo test --workspace` 139 passed / 0 failed (the reported flaky
  `rcli_agent_run_registers_and_connects_a_device` did not fail this run), `cargo clippy --workspace --all-targets`
  clean, `cd dashboard && npx tsc --noEmit` clean. Playwright run here (`DOCKER_HOST` = Colima, ports 18086/3106):
  `server-settings.spec.ts` + `admin.spec.ts`, 13 passed / 0 failed — confirms admin endpoints that this ticket
  re-gated still work for an Admin.
- Not verified by the merger: the OS keychain (only the `0600` file fallback exists, as the ticket allows) and a
  real Windows named pipe, both `shortcut:` comments; the interactive prompt path (`rcli connect` without
  `--password`), since tests pass `--password`; the static Device Password UI in the Dashboard (tickets 09/11).

- code-review fix: a remembered Device Password is no longer plaintext in the Controller config file — `config::remember` now seals the `Remembered` JSON with the Agent's `FileSecretStore` (AES-256-GCM, key file under `passwords/`), `remember`/`forget` delete the old plaintext `<device>.json`, and a unit test asserts no plaintext on disk. Also `rcli agent info` no longer shows the Device Settings to non-root callers (spec: only the ID, status and controllers); they moved behind the root gate with the Server Key and unlock password, covered in `crates/agent/src/ipc.rs` tests.

- final code-review fix (P1, P4, P7): remembered Device Passwords now use the Linux Secret Service (`keyring`
  `sync-secret-service`) instead of always bailing to the 0600 file; when no Secret Service answers (headless
  Controller, cron, `--config-dir` tests) `remember` keeps the 0600 fallback. A Linux-only test points
  `DBUS_SESSION_BUS_ADDRESS` at a dead socket and asserts a clean error rather than a panic or a real keychain write;
  the file-fallback round-trip test still runs everywhere. `rcli setup` now requires `https://` to another host: the
  invented `--insecure-http` flag is gone, loopback http stays as the documented test/same-computer exception, and
  ADR 0003 + docs/usage.md say the same. The generated Device Password alphabet dropped `o` and `L`; the test now
  asserts the whole alphabet and the testkit IPC test rejects the whole `0Oo1lIL` set. Human step (desktop Linux):
  `rcli connect <id>` then `secret-tool search service rcli` (or Seahorse) shows the entry; on a headless host the
  0600 password file remains.

- code-review fix (round 4): `rcli setup` asks for the Server Address and Server Key when `--server`/`--key` are left
  out (the key without echo), as story 17 says. A global `--server` now gets the same https-or-loopback rule as
  `rcli setup` for every command (`connect`, `login`, Device commands), so it cannot send the saved Server Key or a
  remembered Device Password over plain http.
