# 13: `rcli upgrade`

**What to build:** `rcli upgrade` downloads the build currently rolled out for its OS/arch, verifies the signature with the pinned update key, and replaces the running binary. Run as root on an installed Device, it upgrades the service binary and restarts the service right away. See `.scratch/easy-setup/spec.md`.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] A Controller-only machine upgrades itself
- [ ] A bad signature is rejected and the old binary is kept
- [ ] An installed Device upgrades and restarts its service
