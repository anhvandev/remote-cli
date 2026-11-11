# 04: `rcli install` and `rcli uninstall` make a machine both a Device and a Controller

**What to build:** `rcli install --server <Server Address> --key <Server Key>` (needs sudo/UAC) installs the Agent, registers, prints the Device ID and Device Password, and writes the Controller configuration machine-wide, read-only for every local user: Server Address, Server Key, pinned update key and the API Address the Server returned (kept current as the Agent receives it). Any local user can then run `rcli connect <Device ID>` and `rcli mcp` with no further setup. Remembered Device Passwords stay per user. With no API Address on the Server yet, the install still succeeds and Controller commands say clearly that the Server has no API Address. `rcli uninstall` removes the Agent and the machine-wide configuration and keeps each user's remembered Device Passwords. `rcli setup`, `rcli agent install` and `rcli agent uninstall` are removed with no alias (ADR 0006). See `.scratch/setup-v2/spec.md`.

**Blocked by:** 02, 03

**Status:** ready-for-agent

- [ ] After `rcli install`, `rcli connect` and `rcli mcp` work for any local user without further setup
- [ ] One local user cannot read another's remembered Device Passwords
- [ ] With no API Address yet, install succeeds and Controller commands give a clear message
- [ ] A wrong Server Key fails the install immediately and leaves nothing behind
- [ ] `rcli uninstall` removes the Agent and the machine-wide configuration and keeps remembered Device Passwords
- [ ] `rcli setup`, `rcli agent install` and `rcli agent uninstall` no longer exist
- [ ] `cargo test --workspace` and clippy clean

## Comments
