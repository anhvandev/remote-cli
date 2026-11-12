# 05: Controller warning below the Minimum Version

**What to build:** A Controller (CLI or MCP) whose version is below the Server's Minimum Version still runs every command, but prints one stderr line: "Server recommends rcli ≥ Y, run: rcli update". It prints this at most once per process. The Server announces the Minimum Version in a header on every `/api/v1` response.

**Blocked by:** 04

**Status:** resolved

- [ ] With a Minimum Version above the Controller's version, a command succeeds and stderr carries the warning once
- [ ] With no Minimum Version, or one at or below the Controller's version, there is no warning
