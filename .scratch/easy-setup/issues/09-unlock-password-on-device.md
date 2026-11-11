# 09: Unlock password stored on the Device

**What to build:** Move unlock passwords from the Server's Secrets Vault to the Agent's Secret Store, set with `sudo rcli agent unlock-password set|clear`. `unlock` types the password stored on the Device. Remove the unlock-password storage, its endpoints, `rcli unlock-password` and the Dashboard UI from the Server side. The master key stays for the CA key and TOTP secrets. See `.scratch/easy-setup/spec.md`, ADR 0004.

**Blocked by:** 06

**Status:** resolved

- [x] `unlock` works with a password set on the Device and reports clearly when none is set
- [x] The password never leaves the Device and is masked in audit
- [x] No unlock-password storage, endpoint, command or UI remains on the Server side

## Comments

- Implemented on branch `t/09`. The unlock password now lives only in the Agent's Secret Store (`crates/agent/src/secret_store.rs`), under the name `unlock_password`, next to `device_password`.
- **Setting it on the Device.** `sudo rcli agent unlock-password set --password …` / `… clear` are new local-IPC commands (`unlock-password.set` / `unlock-password.clear` in `crates/agent/src/ipc.rs`), gated like `password.set`: root/Administrator only, non-root callers get "this requires root". `SecretStore` gained `remove` so `clear` really deletes the secret (it used to only overwrite).
- **`unlock` types it on the Device.** The Server's `unlock` handler (`api/actions.rs`) now sends `ActRequest { unlock: true }` with **no** actions and **no** password; the Agent's `unlock_actions` reads the stored password and expands it to `[TypeText(password), Key("enter")]` inside the Agent, so the password never crosses the wire. With none stored the Agent answers `unlock_password_not_set` (409), surfaced unchanged through the API and `rcli unlock`. The Control Lock requirement is unchanged. The audit entry is `unlock` with `params: {}` — the password is never in the request, the Observation, the response or the audit row.
- **Server side removed.** Deleted `vault.rs`, `api/vault.rs`, the `unlock_passwords` table/migration, the admin `rcli unlock-password` command (and its `admin_only` entry), and the Dashboard unlock-password UI (admin actions/data/pages, the `UnlockPassword` API type, and the Playwright test).
- **Master key stays.** `MasterKey` moved from the old `vault.rs` to `crates/server/src/master_key.rs`; it still seals the CA private key (`pki.rs`) and TOTP secrets (`auth.rs`) via `RCLI_MASTER_KEY`, which is unchanged. `crates/testkit/tests/master_key.rs` (was `vault.rs`) keeps that coverage.
- **Tests** (`crates/testkit/tests/unlock_password.rs`, driven via `rcli` in-process + the Controller API): unlock with a stored password types it and the audit has no params; `unlock_password_not_set` when cleared (Agent types nothing); each Device types its own password and replacement keeps the newest; non-root `set`/`clear` refused. `cargo test --workspace` 177 passed / 0 failed; `cargo clippy --workspace --all-targets` clean; `npx tsc --noEmit` clean; Playwright `tests/admin.spec.ts` 9 passed (the unlock-password test removed).
- Merged into `easy-setup` (fast-forward to `f1b46bb`). Merge-review fixes: `docs/deploy.md` still claimed `RCLI_MASTER_KEY` "mã hoá mật khẩu unlock" and named the removed `vault_unavailable` error — now it documents the CA key and TOTP secrets only; restored a lost newline in `dashboard/lib/api.ts`.
- Verified on the merged integration tree: `cargo test --workspace` 177 passed / 0 failed; `cargo clippy --workspace --all-targets` clean; `cd dashboard && npx tsc --noEmit` clean; Playwright `tests/admin.spec.ts` 9 passed (ports 18092/3112).
- Not verified: a real OS lock/login screen (FakePlatform only; tickets 15+ adapters). Left in place: the unused `Status::unlock_password()` accessor in `crates/agent/src/ipc.rs` — likely the seam for the ticket 14 GUI.

- code-review fix (round 4): the unlock password is never read back. `info` (root) reports only
  `unlock_password_set`, and the GUI's Reveal shows the Server Key and "an unlock password is set" / "none is set".
