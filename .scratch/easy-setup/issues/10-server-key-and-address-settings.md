# 10: Server Settings: replace the Server Key, set the Server Address

**What to build:** An Admin-only Server Settings page in the Dashboard, plus `rcli server-key rotate` and `rcli server-address set`. Replacing the Server Key changes only its secret, so registered Devices stay Online and new installs and Controllers need the new key. The Server Address replaces `RCLI_AGENT_HOSTNAMES`, is kept in the Server's storage, and reissues the Server's TLS certificate when changed. See `.scratch/easy-setup/spec.md` (Server Settings).

**Blocked by:** 02

**Status:** ready-for-agent

- [x] After a key replacement, registered Devices stay Online and the old key no longer registers or connects
- [x] Changing the Server Address reissues the certificate and Agents reconnect on the new name
- [x] Operators cannot see the page or use the commands
- [x] Playwright test covers the page

## Comments

- Implemented on branch `t/10`. `crates/server/src/settings.rs` holds the two settings in memory and in
  `server_ca`: the Server Key (reusing ticket 02's row) and a new `server_address` column (migration `10001`),
  seeded from `RCLI_AGENT_HOSTNAMES` on first start and edited from then on. `AppState.server_key` became
  `AppState.settings`, and registration reads the secret from it.
- `GET /api/v1/server-settings` (admin) returns the key, CA fingerprint and address;
  `POST /api/v1/server-settings/server-key/rotate` replaces only the secret; `PUT /api/v1/server-settings/server-address`
  stores the address and reissues the TLS certificate. The acceptor is now cached in `AppState.tls` and rebuilt on
  a change, so new connections get the new certificate without a restart. `GET /api/v1/server-key` and the Agent Key
  flow are unchanged.
- CLI: `server-key rotate`, `server-address show|set`, admin-only like `server-key show`.
- Dashboard: Admin-only `/admin/settings` (view and replace the Server Key, set the Server Address) with a card on
  the Quản trị root and a sidebar entry; operators see the existing "Chỉ Admin" refusal.
- Tests: `crates/testkit/tests/settings.rs` (read/refuse, rotation leaves a registered Device Online while the old
  key stops registering, address reissues the certificate so a new name connects and operators are refused) and a
  unit test for address parsing. Playwright `dashboard/tests/server-settings.spec.ts`: replace the key (Devices stay
  Online, fingerprint unchanged), change the address and see it persist, operators see no page or nav entry.
- Not verified: "Agents reconnect on the new name" is covered as a fresh connection through the new name; a Server
  cannot force a live Agent to adopt a name it does not have, and Agent-side `server set` is ticket 06.
