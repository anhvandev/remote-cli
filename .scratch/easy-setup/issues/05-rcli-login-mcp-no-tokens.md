# 05: `rcli login`, MCP from config, no env or API tokens, `update` → `rollout`

**What to build:** `rcli login` signs in with a Dashboard account (password + TOTP) and stores the session in the CLI config. `rcli logout` clears it. Admin commands (`user`, `rollout`, `audit`, `server-key`, Device rename/delete) use that session. `rcli mcp` reads the saved config and remembered passwords. Remove user-created API tokens (`rcli token`, their endpoints, the Dashboard UI) and the `RCLI_SERVER` / `RCLI_TOKEN` env vars. Sessions are already expiring API tokens, so the test system's user helper switches to signing in and its callers stay unchanged. Rename `rcli update …` to `rcli rollout …`. See `.scratch/easy-setup/spec.md`.

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] Admin commands work after `rcli login` and fail with a clear 'run rcli login' error otherwise
- [ ] Operators get a clear refusal for admin commands
- [ ] `rcli mcp` works from saved config with no env vars
- [ ] No user-created API token command, endpoint or UI remains
- [ ] `rcli rollout …` replaces `rcli update …`
