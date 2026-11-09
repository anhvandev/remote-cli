# 02: Server Key and self-registration with a Device ID

**What to build:** The Server generates a Server Key (CA fingerprint + secret) on first start. `rcli agent run --server <addr> --key <key>` registers the Device with no Enrollment Code: the Agent checks the Server's CA against the fingerprint, presents the secret, and gets its mTLS certificate, a random unique 9-digit Device ID and the update-signing key, which it pins. Admins read the key with `rcli server-key show`. `rcli devices` and the Dashboard show the Device ID. Remove Enrollment Codes everywhere (Server, `rcli enrollment-code`, the Dashboard Enrollment page, storage) and the `--ca`, `--ca-fingerprint`, `--enroll` and `--update-key` flags. No data migration. The test-system helper becomes "read the Server Key and register". See `.scratch/easy-setup/spec.md`, ADR 0003.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Registration succeeds with the right key; fails clearly with a wrong secret or a fingerprint mismatch
- [ ] The Device gets a 9-digit Device ID shown in the CLI and Dashboard
- [ ] The update key is pinned at registration; updates signed with another key are rejected
- [ ] No Enrollment Code command, page, endpoint or table remains
