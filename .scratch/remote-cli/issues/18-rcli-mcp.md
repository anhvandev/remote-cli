# 18: `rcli mcp`: MCP tools for AI agents

**What to build:** `rcli mcp` serves the core operations as MCP tools, returning Observations as inline images, so Claude Code and other agents can drive Devices directly.

**Blocked by:** 07, 08, 10, 11

**Status:** ready-for-agent

- [ ] Tools: list Devices, Device details/metrics, Control Lock acquire/release/status, act, Action Batch, observe (all modes), Marks, exec, wake, power
- [ ] Observations returned as inline images; Mark tables as structured data
- [ ] Errors (e.g. Control Lock held) are returned as readable tool errors
- [ ] Smoke test drives one full act → observe loop over MCP
