# 03: The Server learns its API Address and hands it to Agents

**What to build:** The Server records its API Address (GLOSSARY) from an Admin's Dashboard request that arrives through a trusted proxy over `https` with a host that is not localhost or a private or loopback IP. Requests over `http`, to localhost or to a private IP never change it. Server Settings (Dashboard and CLI) show the API Address and let an Admin correct it. The Server sends the API Address to an Agent at registration and on every connection, so a correction reaches every Device without touching it (ADR 0006). See `.scratch/setup-v2/spec.md`.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] An Admin's `https` request with a public host sets the API Address
- [ ] `http`, localhost, private-IP and untrusted-proxy requests leave it unchanged
- [ ] An Admin can read and correct it in Server Settings
- [ ] A registering Agent receives it; a connected Agent receives a corrected value on its next connection
- [ ] `cargo test --workspace`, clippy and `cd dashboard && npx tsc --noEmit` clean; Playwright server settings spec covers the correction

## Comments
