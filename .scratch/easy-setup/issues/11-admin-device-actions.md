# 11: Admin Device actions on the Dashboard: ID, name, new password

**What to build:** On a Device's page, Admins can change the Device ID (9 digits, unique), rename it, and set a new Device Password, which is pushed to the Device's Secret Store. Admins never see the current password. This is how headless Devices are managed remotely. See `.scratch/easy-setup/spec.md` (Dashboard).

**Blocked by:** 06

**Status:** ready-for-agent

- [x] Admin can change ID (duplicate refused), name and set a new password; the Device picks them up live
- [x] Operators do not see these actions
- [x] Playwright test covers them

## Comments

- Implemented on branch `t/11`. The Server-initiated equivalent of ticket 06: `PATCH /api/v1/devices/{device}` takes any
  of `name`, `device_id` and `device_password` (Admin only). Rename is Server-side only; a new Device ID and Device
  Password go to the Device over the live TLS connection and **only then** are stored on the Server, so a refusal
  leaves both sides unchanged.
- **Protocol.** Reused ticket 06's `IdentityUpdate` instead of a parallel message: `ServerMessage.identity_change = 22`
  carries it Server → Agent, and `AgentMessage.identity_updated = 22` carries the Agent's `IdentityUpdated` answer.
  The field numbers `IdentityUpdate`/`IdentityUpdated` already had (20) stay as they were, so no existing wire format
  changes. The Agent's handler (`server_identity_change`) writes a new Device Password to its Secret Store, keeps a
  new Device ID (and rewrites `credentials.json` through the existing `Status::set_device_id`), and answers with the
  Device ID in effect or an error.
- **Checks before pushing.** The password must be `>= 8` characters (same rule as ticket 06, checked on the Server
  *and* the Agent). The Device ID is validated with `registry::validate_device_number` (9 ASCII digits, `UNIQUE` with
  "already used by another Device") *before* the push, so a duplicate is a `409 device_id_taken` and a malformed one a
  `400`. A Device that is already Offline gets `409 device_not_connected` from `Agents::request` (no queueing).
  `registry::set_device_number` now reuses the validator.
- **Offline Device (deliberate choice).** A password or ID change is *refused* with `409 device_not_connected` and
  nothing changes: both are pushed to the Device, and storing them on the Server only would leave the Device holding
  the old ID/password with no way to learn the new one (queueing is out of scope). A rename needs no push and works
  while Offline. The Dashboard card says the Device must be Online.
- **Never readable.** No endpoint returns a password: the existing `device_password_hash` column keeps only an argon2
  hash, the PATCH reply is the ordinary `Device` JSON, and the audit entry for `device.password` has empty params. The
  test asserts neither the reply nor the audit log contains the new password.
- **Dashboard.** A new `AdminActions` card on every Device page (`app/(app)/devices/[id]/admin-actions.tsx`) with the
  three forms, rendered only when `me.role === "admin"`. Operators see no card and the Server refuses them
  (`403 admin_required`) — the Playwright test calls the API directly with the operator's cookie to prove it.
- **Tests.** `crates/testkit/tests/admin_device.rs` (5): password pushed live to the Secret Store, old password stops
  authenticating and the new one works, no password in the reply or audit, short password refused; Device ID duplicate
  (`409`) / malformed (`400`) / live change, old ID 404 and a Controller connects by the new one; rename (empty
  refused); Operator 403 on all three; Offline rename works while ID/password are `409`. Playwright
  `dashboard/tests/device-admin.spec.ts` (2): admin renames, sets a password (never shown in the page, nor after a
  reload), changes the ID and sees a duplicate refused; operators see no card and the API refuses them.
- The devstack gained a `kilo` Device for these tests (the specs run in parallel and rename/renumber it). The
  literal `5 Online` counts in `admin.spec.ts`, `fleet-filter.spec.ts` and `signin-fleet.spec.ts` became `6 Online`.
- Verification: `cargo test --workspace` 161 passed / 0 failed; `cargo clippy --workspace --all-targets` clean;
  `cd dashboard && npx tsc --noEmit` clean; the **whole** Playwright suite run on ports 18090/3110: 38 passed /
  0 failed (the new spec is 2 of them).
- Not verified: the Windows named pipe (unchanged ticket 03 `shortcut:`); a real Device changing its ID while a
  Controller holds an open stream (the ticket 06 tests cover streams surviving a password change; an ID change
  re-authenticates only new connections, which the test shows by connecting with the new ID).
- `shortcut:` one, on `api::devices::push`: a reply lost after the Agent applied a change leaves the two sides
  disagreeing (the Server keeps the old ID/password); re-read the Device's identity on the next connection if that
  race ever shows up. Reused ticket 06's `IdentityUpdate` "empty field = unchanged" convention, which is why each
  change is pushed as its own message.
- Invented where the spec is silent: `ServerMessage.identity_change` / `AgentMessage.identity_updated` as the
  Server-initiated mirror of ticket 06's messages; the Offline-Device refusal (`409 device_not_connected`, no
  queueing) and rename working Offline; `audit` action names `device.id` and `device.password`; the Dashboard card
  living on the Device page rather than in `/admin/devices`.
