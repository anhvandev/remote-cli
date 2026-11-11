# 02: Controller API through the Dashboard URL

**What to build:** The Dashboard forwards the whole Controller API to the internal Server, including streaming responses (the shell stream, Control Lock watching), the same way it already proxies the install scripts and the web terminal. A Controller pointed at the Dashboard's public URL can use every `rcli` and `rcli mcp` command. The deploy docs say only the Dashboard port and the Agent port face the internet; the Controller API port stays internal (ADR 0006). See `.scratch/setup-v2/spec.md`.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [ ] `rcli` commands, `rcli mcp` tools and the shell stream work against the Dashboard URL
- [ ] Holding the Control Lock through the Dashboard URL still frees the lock 60 s after the holder disconnects
- [ ] The Controller's real IP still reaches the Server (rate limiting and audit are per Controller, not per Dashboard)
- [ ] The deploy docs no longer expose the Controller API port
- [ ] `cargo test --workspace`, clippy and `cd dashboard && npx tsc --noEmit` clean; Playwright terminal spec passes through the forwarded API

## Comments
