# 02: Users, roles and API tokens

**What to build:** Admins manage users with the role admin or operator, and issue per-user API tokens. The CLI and AI agents authenticate with these tokens. The bootstrap token from ticket 01 becomes a first-run mechanism only.

**Blocked by:** 01

**Status:** resolved

- [ ] Admin can create, list and disable users and issue/revoke API tokens via CLI
- [ ] Every Controller API call requires a valid token; revoked tokens are rejected
- [ ] Operators are refused admin-only operations with a typed error
- [ ] Tests cover role enforcement through the Controller API

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
