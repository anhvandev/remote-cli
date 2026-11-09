# 02: Server Key and self-registration with a Device ID

**What to build:** The Server generates a Server Key (CA fingerprint + secret) on first start. `rcli agent run --server <addr> --key <key>` registers the Device with no Enrollment Code: the Agent checks the Server's CA against the fingerprint, presents the secret, and gets its mTLS certificate, a random unique 9-digit Device ID and the update-signing key, which it pins. Admins read the key with `rcli server-key show`. `rcli devices` and the Dashboard show the Device ID. Remove Enrollment Codes everywhere (Server, `rcli enrollment-code`, the Dashboard Enrollment page, storage) and the `--ca`, `--ca-fingerprint`, `--enroll` and `--update-key` flags. No data migration. The test-system helper becomes "read the Server Key and register". See `.scratch/easy-setup/spec.md`, ADR 0003.

**Blocked by:** 01

**Status:** ready-for-agent

- [x] Registration succeeds with the right key; fails clearly with a wrong secret or a fingerprint mismatch
- [x] The Device gets a 9-digit Device ID shown in the CLI and Dashboard
- [x] The update key is pinned at registration; updates signed with another key are rejected
- [x] No Enrollment Code command, page, endpoint or table remains

## Comments

- Implemented on branch `t/02`. Server Key `rk_<CA fingerprint>_<secret>` (secret random, stored in `server_ca`), issued on first start and read by admins at `GET /api/v1/server-key` / `rcli server-key show`.
- Registration (`RegisterRequest`/`RegisterResponse`, replacing `Enroll*`) over the Agent TLS port: the Agent pins the CA from the key's fingerprint (custom rustls verifier), presents the secret, and gets its mTLS cert, a random unique 9-digit Device ID and the update public key, which it stores in its credentials. The Server now sends its CA in the TLS chain and in the registration reply.
- The UUID stays the storage key (`devices.id`) alongside a new `device_number`; the registry exposes it as `device_id` and accepts it (plus the UUID and the name) in `resolve`. CLI `devices`, audit entries and the Dashboard show the 9-digit ID.
- Enrollment Codes removed: `enrollment.rs`, `api/enrollment.rs`, the `enrollment-codes` endpoint, `rcli enrollment-code`, the Dashboard page/nav/overview card, the test helpers and the `enrollment_codes` table (migration `16002`). `--ca`, `--ca-fingerprint`, `--enroll`, `--update-key` replaced by `--key`; the Server update public key comes from `RCLI_UPDATE_PUBLIC_KEY`.
- Verified on the merge of `easy-setup`: `cargo test --workspace` 119 passed / 0 failed, `cargo clippy --workspace --all-targets` clean. Dashboard Playwright not run here (no `node_modules` in this worktree); `admin.spec.ts` updated for the removed Enrollment Code page and the fleet Device ID.
