# 06: Change Device Password, ID, name and Server from the Device

**What to build:** Extend the local IPC with root-only `rcli agent password set | id set | name set | server set`. Each applies to the running service at once, without a restart. Password, ID and name changes go to the Server: the ID is checked for uniqueness, and a new password must be at least 8 characters. Existing sessions survive a password change. Changing the Server Address or Key reconnects the Agent. See `.scratch/easy-setup/spec.md` (Local IPC).

**Blocked by:** 03

**Status:** resolved

- [x] Each command applies live; tests drive them with `rcli agent …` in-process
- [x] Non-root callers are refused every change
- [x] A duplicate Device ID is refused with a clear error; Controllers using the old ID get a clear 'not found'
- [x] After a password change new connections need the new password and open sessions continue
- [x] Changing Server Address/Key reconnects without a restart

## Comments

- Implemented on branch `t/06`. The Agent's IPC (`crates/agent/src/ipc.rs`) gained `password.set`, `id.set`, `name.set`
  and `server.set` next to ticket 03's `info` and `password.show`. One permission rule: `info` is for any local user;
  everything else is refused unless the peer credential says uid 0 (the same gate ticket 03 built).
- **Going to the Server.** Password, ID and name changes leave the Agent on the live TLS connection as a new
  `AgentMessage.identity_update` (`IdentityUpdate { request_id, device_password, device_id, name }`, an empty field
  means unchanged). The Server (`agent_link::identity_update`) checks everything first (9 digits, uniqueness,
  1-255 name, >= 8 characters) and applies it in the same handler, then answers `ServerMessage.identity_updated`
  with the Device ID in effect or an error. The Agent's `session` keeps a `request_id -> oneshot` map and resolves
  each `rcli agent …` call from it. The password goes to both places: the Server stores a new argon2 hash
  (`registry::set_device_password`), the Agent writes the plaintext to its Secret Store. No connection is touched, so
  open streams (a held `lock/watch`, a live shell) keep running; only a new connection re-authenticates.
- **Changing the Server.** `Status` owns a `watch::channel<ServerEndpoint>`; `server.set` replaces it, the running
  `session` selects on its `changed()` and returns, and `run` reconnects with the new endpoint at once (backoff
  reset) instead of waiting. No process restart. The CLI validates the key shape (`rk_<64 hex>_<secret>`) before
  sending.
- **Device ID.** `registry::set_device_number` enforces 9 ASCII digits and `devices.device_number` UNIQUE with a
  clear "already used by another Device". The Agent updates `Status` (so `rcli agent info` shows it at once) and, in
  `rcli agent run`, rewrites `credentials.json` through `Status::set_credentials_path` so the new ID survives a
  restart. A Controller still sending the old ID gets `401` from the connection (it resolves the ID) and an Admin
  `GET /devices/<old>` gets `device_not_found` (404).
- **Refusal gate.** The real socket check (`ipc::serve`) is exercised by `ipc.rs`'s
  `password_show_is_refused_to_non_root` and by `every_change_is_refused_to_a_non_root_caller`. The change tests
  themselves need root behaviour and cannot be root in the harness, so `TestSystem::start_agent_trusting_socket`
  serves `ipc::serve_trusting_all` (same `handle`, peer check replaced by `true`); that is the only test-only piece
  and it is documented on the helper. As a unit test `ipc::tests::only_root_may_show_or_change_the_device` covers
  every command's non-root refusal directly.
- **Tests.** `crates/testkit/tests/device_identity.rs`: password change (8-char gate, old password stops working for
  new requests while an open `lock/watch` stream keeps streaming, Secret Store holds the new one), Device ID
  (duplicate and malformed refused, live under the new ID, old ID 404 `device_not_found`), name (applies, resolves,
  empty refused), Server (malformed key refused, dead address disconnects, back to the real address reconnects with
  no restart and the Device is Online again), and the non-root gate for all four commands. Two unit tests in
  `ipc.rs` cover the dispatch gate and the short-password refusal without a Server.
- Verification: `cargo test --workspace` 145 passed / 0 failed, `cargo clippy --workspace --all-targets` clean,
  `cd dashboard && npx tsc --noEmit` clean. The dashboard is untouched (no display change), so no Playwright spec
  was affected.
- Not verified: the Windows named pipe (still the ticket 03 `shortcut:`; the new commands reuse the same `handle`).
  A real Device moving to *another* Server was not driven end to end: the test reconnects to the same Server after a
  dead address because a second Server needs the Device's certificate and row moved across, which is Server-admin
  work (ticket 11) rather than this one; the reconnect path itself (drop the session, connect with the new endpoint,
  no restart) is what the test covers.
- Invented where the spec is silent: the `identity_update`/`identity_updated` proto messages and their "empty means
  unchanged" convention; `Status::set_credentials_path` rewriting `credentials.json` so a new Device ID survives a
  restart; `password set` refusing a password shorter than 8 at the Agent *and* the Server (belt and braces); the
  "not connected to the Server" message when a change arrives while the Agent is Offline (nothing is applied, so the
  caller can retry).
- Merged into `easy-setup` (merge `0bc6abc`). **One conflict**: `crates/server/src/api/mod.rs`, where t/06 had
  changed the password-connection error text while ticket 04 had turned the same `Option` check into a
  `Result<_, ControllerError>` (lockout handling). Resolved by keeping 04's structure and t/06's clearer message on
  the `Unauthorized` arm: a wrong Device ID now reads "wrong Server Key or Device Password, or no Device with this
  Device ID"; the `Locked` arm (429 `too_many_attempts`, `locked_until`) is untouched. Everything else
  (`agent_link.rs`, `registry.rs`, `agent.proto`, `agent/src/lib.rs`, `cli/src/*`, `testkit/src/lib.rs`) merged
  cleanly because tickets 04/05 did not touch them; `testkit/src/lib.rs` kept both 04's `start_dual_stack`/05's
  session helpers and t/06's `start_agent_trusting_socket`.
- Merger verification on `0bc6abc` (run alone): `cargo test --workspace` 156 passed / 0 failed; the at-risk suites
  re-run explicitly — `device_identity` 5, `controller` 3, `rate_limit` 9, `auth` 6, `sessions` 6, all pass.
  `cargo clippy --workspace --all-targets` clean; `cd dashboard && npx tsc --noEmit` clean. Playwright not run: t/06
  touches no dashboard file (the dashboard diffs in the merge range are all from ticket 05's token removal).
- Review points (merger):
  - **ID uniqueness** is enforced by both a `SELECT EXISTS ... AND id <> $2` check and the existing
    `devices_device_number_key` UNIQUE constraint (`crates/server/migrations/16002`), so a duplicate is a clear
    "already used by another Device", not a 500. Tested.
  - **Old ID after a change**: the test asserts a Controller using the old ID gets `401` with the "no Device with
    this Device ID" text (the connection path resolves the ID) and an Admin `GET /devices/<old>` gets `404
    device_not_found`. No stale success.
  - **Open sessions survive a password change**: verified by the held `lock/watch` stream still yielding a chunk
    after the change, and by reading the path — the password is only read in `authenticate_controller` when a
    connection opens; nothing re-checks it mid-stream.
  - **Root gate**: `ipc::serve` uses `stream.peer_cred()` (`uid == 0`), and `dispatch` refuses every command except
    `info` before the match. The harness runs as uid 502, so `every_change_is_refused_to_a_non_root_caller` really
    exercised the refusal path, not the trusted one. `serve_trusting_all` is reachable only from `rcli-testkit`
    (a `publish = false` crate nothing in the workspace depends on), never from `rcli` or `rcli-agent`'s own code.
  - **Server change reconnects in place**: `watch::channel` change makes the live session return, `run` re-enters
    `connect` with the new endpoint and skips the backoff; the old connection's sender is removed by the existing
    `same_channel` check, so no double registration. The test drives a dead address then back to the real one and
    sees this same Device Online again.
  - **Scope**: nothing from 07/08/09/11/13 (no dashboard, Terminal User, accept-mode, unlock-password, upgrade).
- Not verified by the merger: a real move to a *different* Server (needs the certificate/row moved, ticket 11);
  the Windows named pipe. Both are recorded above by the implementer.

