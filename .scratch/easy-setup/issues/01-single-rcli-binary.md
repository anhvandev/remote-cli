# 01: One `rcli` binary for Controller and Agent

**What to build:** Prefactor: move the Agent's entry point into the CLI as `rcli agent run`, keeping the Agent as a library, and remove the `rcli-agent` binary. Flags stay as they are for now. The test system and dev stack start Agents through the same entry point. See `.scratch/easy-setup/spec.md` (Implementation Decisions: One binary).

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] `rcli agent run` does everything `rcli-agent` did
- [x] No `rcli-agent` binary or reference to it remains (container image, docs, test system)
- [x] The whole existing test suite passes unchanged

## Comments

- Implemented by `ccs ds` on branch `t/01` (cea4dae). Verified on `easy-setup`: `cargo test --workspace` 114 passed / 0 failed, clippy clean. `rcli-agent` remains only as the library crate name and the `rcli-agent-build` update-signature domain string (changing it would break signatures).
