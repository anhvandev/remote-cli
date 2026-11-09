# 05: `rcli login`, MCP from config, no env or API tokens, `update` → `rollout`

**What to build:** `rcli login` signs in with a Dashboard account (password + TOTP) and stores the session in the CLI config. `rcli logout` clears it. Admin commands (`user`, `rollout`, `audit`, `server-key`, Device rename/delete) use that session. `rcli mcp` reads the saved config and remembered passwords. Remove user-created API tokens (`rcli token`, their endpoints, the Dashboard UI) and the `RCLI_SERVER` / `RCLI_TOKEN` env vars. Sessions are already expiring API tokens, so the test system's user helper switches to signing in and its callers stay unchanged. Rename `rcli update …` to `rcli rollout …`. See `.scratch/easy-setup/spec.md`.

**Blocked by:** 03

**Status:** ready-for-agent

- [x] Admin commands work after `rcli login` and fail with a clear 'run rcli login' error otherwise
- [x] Operators get a clear refusal for admin commands
- [x] `rcli mcp` works from saved config with no env vars
- [x] No user-created API token command, endpoint or UI remains
- [x] `rcli rollout …` replaces `rcli update …`

## Comments

- Implemented on branch `t/05` (d44f070, 5e829ae, e6e2e35). Vertical slices, each with its test first:
  - **`rcli login` / `rcli logout`** (`crates/cli/src/lib.rs`, `config::set_token`): `login <name>` takes the
    Dashboard password and TOTP (asked for when the flags are omitted), posts `/api/v1/sessions` and stores the
    session in `<config dir>/config.json` next to the Server Address/Key. `logout` revokes it (best effort) and
    clears the field. The stored token is what every later command authenticates with.
  - **Admin commands need a session**: `admin_only()` lists Server administration (`user`, `rollout`, `audit`,
    `server-key`, `server-address`, `unlock-password`, Device rename/delete/revoke/merge, `lock force-release`).
    Without a session the CLI refuses before the request with `this command needs an Admin account: run rcli login`;
    with an Operator's session the Server answers `admin_required`, as before. The list mirrors ADR 0004.
  - **API tokens removed**: the `rcli token` command and its `TokenCommand` are gone; `POST /users/{name}/tokens`,
    `GET /tokens`, `POST /tokens/{id}/revoke` and `auth::{issue_token, list_tokens, revoke_token, TokenInfo}` are
    deleted. Sessions (expiring tokens) stay: `authenticate`, `revoke_by_token` and sign-in are unchanged. The
    bootstrap-token rescue path now ignores expired admin sessions (it used to count any unrevoked row, so it would
    have stayed disabled after every session expired).
  - **Dashboard**: the "Token" column, "Tạo token" button and the tokens count on the Quản trị overview are gone,
    along with `act.createToken`/`act.revokeToken` and `ApiToken`. The users page is now "User".
  - **`rcli mcp` from config**: `Api::from_config` is what `rcli mcp` builds, so the Server and the principal come
    from the saved config: a `rcli login` session, else a `rcli connect` Device Password. New
    `TestSystem::mcp_in` covers both with no env vars.
  - **`update` → `rollout`**: `rcli rollout keygen|publish|<version> --all|--device` replaces `rcli update …`.
    `update.rs` is renamed `rollout.rs`; the Server's `/agent-builds/.../rollout` endpoint keeps its name.
  - **Test system**: `create_user` now creates the user, sets the Dashboard password (`TestSystem::PASSWORD`) and
    signs in, returning the session token, so every caller stays unchanged. Sign-in needs no admin rights, which
    keeps the helper usable from within tests. Sessions expire after 12 h, so tests that move the clock past that
    call `sign_in_admin()` (history, audit, sessions) and the dev stack renews once after it jumps to real time.
- Verification: `cargo test --workspace` 141 passed / 0 failed; `cargo clippy --workspace --all-targets` and
  `cd dashboard && npx tsc --noEmit` clean. Playwright `tests/admin.spec.ts` 10/10 passed on ports 18085/3105
  against the dev stack (the spec's token step was removed; the rest is unchanged).
- Not verified: the interactive password/TOTP prompts (tests pass `--password`/`--totp`); the Dashboard sign-out
  cookie path is unchanged by this ticket.
- Invented where the spec is silent: the `admin_only` local refusal wording (the Server remains the authority);
  `logout` revoking best-effort before clearing the saved session.

