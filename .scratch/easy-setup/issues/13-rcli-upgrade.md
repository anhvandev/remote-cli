# 13: `rcli upgrade`

**What to build:** `rcli upgrade` downloads the build currently rolled out for its OS/arch, verifies the signature with the pinned update key, and replaces the running binary. Run as root on an installed Device, it upgrades the service binary and restarts the service right away. See `.scratch/easy-setup/spec.md`.

**Blocked by:** 05

**Status:** resolved

- [x] A Controller-only machine upgrades itself
- [x] A bad signature is rejected and the old binary is kept
- [x] An installed Device upgrades and restarts its service

## Comments

- Implemented on branch `t/13`. Vertical slices, each with its test first (`crates/testkit/tests/upgrade.rs`):
  - **Fetch + verify + replace**: the Server gained a public `GET /api/v1/agent-builds/{target}` (in
    `crates/server/src/api/updates.rs`, backed by `updates::latest`) that returns the most recently published
    signed build for a target, with `X-Rcli-Version` and `X-Rcli-Signature` headers and the bytes as the body.
    `rcli upgrade` (`crates/cli/src/upgrade.rs`, wired in `crates/cli/src/lib.rs`) fetches it, verifies the Ed25519
    signature with the `update_key` pinned at `rcli setup` via the existing `rcli_protocol::update::verify`, and
    replaces the binary atomically (temp file + `rename`, `0755`). Bad signature aborts **before** any write.
  - **Newest, not "per-Device"**: a Controller-only machine has no `devices` row, so "the build rolled out for its
    OS/arch" cannot be the per-Device `update_version`; the newest published build for the target stands in
    (`ORDER BY uploaded_at DESC, version DESC`). Test `upgrade_fetches_the_newest_published_build_for_this_target`
    publishes `2.0.0` then `1.0.0` a minute later and asserts `1.0.0` wins.
  - **Service restart**: as root on Linux, `service_to_restart` checks `systemctl cat rcli-agent` *before* the swap
    (a refusal must not leave a new binary unstarted), then `systemctl restart rcli-agent`. Off Linux, or as a
    normal user, no service is involved. Clear error: `no rcli-agent service is installed, so the new binary was
    not restarted`.
  - **Pin caveat (the shared context's "Open security caveat") — fixed cheaply**: the update key is still fetched
    over plain HTTP, but `/api/v1/update-key` now also returns `mac`, an HMAC-SHA256 of the hex key under the Server
    Key's secret (`rcli_protocol::update::pin_mac`); `rcli setup` recomputes it with the secret the user typed and
    refuses to pin on mismatch (`config::check_pin`). An attacker rewriting `/api/v1/server-ca` and
    `/api/v1/update-key` together cannot produce the MAC without the secret. This is not TLS and does not protect a
    Controller whose Server Key is already leaked, but it closes the on-path rewrite the caveat described. Covered by
    `a_server_that_cannot_prove_the_update_key_is_refused` (a local impostor serves the real CA and a forged key).
- Not verified: the "installed Device" case needs a real Linux systemd host. `service_to_restart`/`restart` are
  plain `std::process::Command` calls guarded by `cfg!(target_os = "linux") && is_root()`; they compile on macOS
  and on Linux (checked with `rustc` in isolation, since the crate's `ring` dependency needs a Linux C toolchain
  this machine lacks), but no test drives the root path. The "no service installed" error likewise.
- `shortcut:` comments left:
  - `upgrade.rs` `service_to_restart`: assumes the unit is named `rcli-agent` and runs the binary being replaced;
    ticket 16's `rcli agent install` should record the real unit/path. Also: not exercised here.
  - `upgrade.rs` `replace`: renaming over a running `.exe` fails on Windows; ticket 22's installer must move-aside
    first.
- Invented where the spec is silent: the endpoint shape `GET /api/v1/agent-builds/{target}` and the header names
  carrying version + signature; "newest published build per target" as the meaning of "currently rolled out" for a
  Controller; the hidden `--target` flag (the binary-write seam; never printed in `--help`); restart only when
  UID 0 (not merely "Linux"), so a Controller-only machine never touches systemd.
- `cargo test --workspace`: see the report; `cargo clippy --workspace --all-targets` and
  `cd dashboard && npx tsc --noEmit` clean. Dashboard untouched, so Playwright was not run.

## Comments

- Implemented by a subagent on branch `t/13` (`c171689`, 4 commits). `rcli upgrade` fetches the newest published build
  for its target from `GET /api/v1/agent-builds/{target}`, verifies the Ed25519 signature against the pinned update
  key, and replaces the binary atomically (temp file + rename). No build for the target gives a clear error. On Linux
  as root it checks the `rcli-agent` unit exists before swapping, then restarts it; non-root or non-Linux leaves the
  service alone.
- The update-key pin caveat from ticket 03's review is fixed: `/api/v1/update-key` now also returns
  `mac` = HMAC-SHA256 of the key under the Server Key secret, and `rcli setup` recomputes and rejects a mismatch. An
  on-path attacker rewriting both `/server-ca` and `/update-key` over plain HTTP cannot forge the MAC without the
  secret. Covered by a test with a local impostor serving the real CA and a fake key.
- Verified on the merged integration tree: `cargo test --workspace` green (5 tests in `tests/upgrade.rs`), clippy and
  `tsc` clean. Playwright not applicable (dashboard untouched).
- Not verified: the installed-Device path needs a real Linux machine with systemd; `service_to_restart`/`restart` are
  compile-checked only.
- `shortcut:` (1) `service_to_restart` assumes a unit named `rcli-agent` — ticket 16 must confirm the real name and
  path; (2) `replace` over a running `.exe` fails on Windows — ticket 22 must move the file aside first.

- code-review fix: `rcli upgrade` no longer ignores the rollout. It asks the running Agent over its local IPC for the Device ID (`--socket`/`RCLI_AGENT_SOCKET`, default socket) and requests `GET /api/v1/agent-builds/{target}?device=<id>`; the Server serves the build rolled out to that Device (`devices.update_version`), falling back to the newest published build only when there is no Device row (Controller-only machine) or no rollout yet (`updates::upgrade_build`, test `an_installed_device_upgrades_to_the_build_rolled_out_to_it`). Also "normal user" → "a non-root account" in `service_to_restart`'s comment (glossary: Operator).
