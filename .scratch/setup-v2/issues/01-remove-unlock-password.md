# 01: Remove the stored unlock password

**What to build:** rcli stores no OS login password anywhere (ADR 0005). A Controller gets past a lock or login screen only by typing the password itself with a `type` Action marked `secret`. Remove the unlock password end to end: the Agent typing a stored password, its Secret Store entry and set/clear requests, the "unlock password set" flag in the Agent's info, the Agent GUI section, `rcli agent unlock-password`, `rcli unlock`, the Server's unlock endpoint and its "not set" error, and the `unlock` field of the Agent protocol's act request (its field number stays reserved so mixed versions keep working during a rollout). An upgraded Agent deletes any leftover unlock password from its Secret Store on startup. `rcli mcp`'s instructions tell the AI agent to type the OS password the user gave it as a secret `type` at a lock or login screen, because rcli does not store it. The docs no longer mention unlock passwords. See `.scratch/setup-v2/spec.md`.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] No unlock-password storage, command, endpoint, IPC request, GUI section or protocol field remains; the field number is reserved
- [ ] An Agent that starts with a leftover unlock password in its Secret Store removes it
- [ ] Text typed with `secret`, or while the Device is at a lock or login screen, is still masked in audit
- [ ] The MCP instructions tell the AI agent to type the OS password itself
- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets` clean

## Comments
