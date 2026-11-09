# 01: One `rcli` binary for Controller and Agent

**What to build:** Prefactor: move the Agent's entry point into the CLI as `rcli agent run`, keeping the Agent as a library, and remove the `rcli-agent` binary. Flags stay as they are for now. The test system and dev stack start Agents through the same entry point. See `.scratch/easy-setup/spec.md` (Implementation Decisions: One binary).

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `rcli agent run` does everything `rcli-agent` did
- [ ] No `rcli-agent` binary or reference to it remains (container image, docs, test system)
- [ ] The whole existing test suite passes unchanged
