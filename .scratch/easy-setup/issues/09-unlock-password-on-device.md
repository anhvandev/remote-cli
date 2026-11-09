# 09: Unlock password stored on the Device

**What to build:** Move unlock passwords from the Server's Secrets Vault to the Agent's Secret Store, set with `sudo rcli agent unlock-password set|clear`. `unlock` types the password stored on the Device. Remove the unlock-password storage, its endpoints, `rcli unlock-password` and the Dashboard UI from the Server side. The master key stays for the CA key and TOTP secrets. See `.scratch/easy-setup/spec.md`, ADR 0004.

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] `unlock` works with a password set on the Device and reports clearly when none is set
- [ ] The password never leaves the Device and is masked in audit
- [ ] No unlock-password storage, endpoint, command or UI remains on the Server side
