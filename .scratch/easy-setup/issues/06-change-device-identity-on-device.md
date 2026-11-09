# 06: Change Device Password, ID, name and Server from the Device

**What to build:** Extend the local IPC with root-only `rcli agent password set | id set | name set | server set`. Each applies to the running service at once, without a restart. Password, ID and name changes go to the Server: the ID is checked for uniqueness, and a new password must be at least 8 characters. Existing sessions survive a password change. Changing the Server Address or Key reconnects the Agent. See `.scratch/easy-setup/spec.md` (Local IPC).

**Blocked by:** 03

**Status:** ready-for-agent

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
