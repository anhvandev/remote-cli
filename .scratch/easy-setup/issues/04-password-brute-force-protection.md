# 04: Brute-force protection for Device Passwords

**What to build:** The Server rate-limits wrong Device Passwords per (Device ID, IP): 5 failures lock out for 1 minute, each next lockout doubles, up to 1 hour. Every failure and lockout is audited. The CLI shows a clear message with the remaining lockout time. Must not be cut: it is the real protection behind a weak Server Key (ADR 0003). See `.scratch/easy-setup/spec.md` (Security).

**Blocked by:** 03

**Status:** resolved

- [x] Lockout after 5 failures, doubling up to 1 hour, verified with the injectable clock
- [x] Lockouts are per (Device ID, IP): another IP or another Device is unaffected
- [x] A correct password after the lockout ends works and resets the counter
- [x] Failures and lockouts appear in the audit log

## Comments

- Implemented on branch `t/04`, building on ticket 03's `auth::authenticate_controller`:
  - **Counter storage** (migration `20001`): one row per `(device, ip)` in `device_password_failures` holding
    `failures`, `lockouts`, `locked_until` and `seen_at`. The Device key is its UUID when the Device exists, so
    every way of naming it (9-digit Device ID, UUID, name) shares one bucket; an unknown Device ID is keyed as
    presented.
  - **Limiter** (`auth::authenticate_controller` + `password_failed`): the fifth wrong password starts a 1-minute
    lockout, each later one doubles (1, 2, 4, 8, 16, 32, then capped at 60 minutes), and reaching the limit resets
    the counter so the next round counts from zero. Counting is one atomic SQL upsert: a read-then-write lost
    failures under parallel requests, so a parallel guesser would never have been locked out (a test covers it).
    A pair unseen for 24 h starts over. A correct password deletes the row, so it both works after the lockout and
    resets the count.
  - **HTTP**: a lockout is `429 too_many_attempts` with `locked_until` and a message with the remaining seconds
    (`try again in 60s`), the same shape as the Dashboard sign-in lockout.
  - **CLI**: `rcli connect` (and every password connection) prints the lockout message verbatim instead of the
    "Device ID or Device Password is wrong" text, because a lockout does not mean the password was wrong.
  - **Audit**: every failure is `connect.failed` (`result.error = "unauthorized"`) and every lockout is
    `connect.locked` (`result.locked_until`), both named `<controller hostname> (<IP>)` and attributed to the
    Device (or `00000000-…` for an unknown Device ID) so the Dashboard audit view shows them. An audit failure is
    logged, never turned into a wrong answer to the caller.
- **Device ID enumeration (deliberate choice):** a wrong Device ID is indistinguishable from a wrong password:
  same `401 unauthorized` body, and an unknown Device runs a dummy argon2 verify so response time does not tell
  them apart. Wrong Device IDs are counted too, keyed as presented, so guessing Device IDs is rate-limited; they
  never lock a real Device, because the real Device is keyed by its UUID.
- Verification: `crates/testkit/tests/rate_limit.rs` (9 tests, all through the Controller API or `rcli` in-process
  with `TestSystem::advance`): first lockout and remaining time, doubling to the hour cap, per-(Device, IP)
  isolation using the Server's `::1` side (`TestSystem::start_dual_stack`) for a genuinely different peer IP,
  counter reset on success, audit entries, the CLI message, wrong-ID indistinguishability, and parallel guesses.
  Full `cargo test --workspace` and `cargo clippy --workspace --all-targets` clean; the dashboard is untouched, so
  Playwright was not run.
- `shortcut:` left in `auth.rs`: the Server Key check is not counted (it reveals nothing about passwords, and a
  leaked key still has to pass the Device Password limiter); the failure window is fixed rather than sliding; a
  Server behind a reverse proxy sees the proxy's IP, which makes the limiter shared (and the naming wrong).
- Not verified: a real client behind NAT (all tests use loopback; isolation is proven with `127.0.0.1` vs `::1`).
- Merged into `easy-setup` (merge `8c80bdf`, no conflicts). Diff `e6affcf..t/04` is only ticket 04 (limiter,
  migration `20001`, CLI message, testkit dual-stack helper, tests); nothing from 05's login/token removal or 06.
  Migration `20001` sorts after every existing migration and applies cleanly on a fresh database.
- Security review (merger):
  - **Parallel guessing:** counting is a single `INSERT … ON CONFLICT DO UPDATE`, so ten simultaneous wrong
    guesses all count (the `parallel_guesses_still_reach_the_lockout` test proves the pair is locked after one
    burst). The earlier read-then-write race is gone.
  - **Device ID variations:** an existing Device is bucketed by its UUID, so Device ID, UUID and name share one
    counter; an unknown ID is bucketed as presented and counted, so renaming the target does not dodge the limit
    and never locks a real Device.
  - **`X-Forwarded-For` / other headers:** no header is read for the IP. `ConnectInfo` (the TCP peer) is the only
    source, so a client cannot spoof its bucket with a header.
  - **Many connections:** each connection is just another HTTP request counted by the same pair row; nothing is
    per-connection.
  - **Reset and cap:** a correct password deletes the row (works after the lockout and resets the count);
    lockouts are 1, 2, 4, 8, 16, 32, then `LEAST(…, 1 h)`, verified at every step with the injectable clock.
  - **Audit:** every failure writes `connect.failed` and every lockout `connect.locked` with only `locked_until`
    in `result` — the attempted password is never in `params`/`result` (checked against the audit writer; params
    are `{}`). An audit error is logged and never surfaces to the caller.
  - **Timing:** an unknown Device / revoked Device / empty hash runs `dummy_password_check`, a real argon2 verify
    against a fixed hash, so the answer time does not reveal whether the Device exists.
  - **Scope:** only the password path (`Caller` extractor) calls the limiter. A bearer User/Admin session never
    reaches it, and the bucket includes the IP, so another IP or another Device is unaffected (tested with the
    Server's `127.0.0.1` vs `::1` sides).
- Fix made by the merger: the reverse-proxy limit was only in this ticket's prose, not in the code. Added the
  `shortcut:` comment at the IP source in `crates/server/src/api/mod.rs` (the peer IP is used; trust
  `X-Forwarded-For` only once a known proxy hop is configured). The other two `shortcut:` comments were already in
  `auth.rs`.
- Verified on the merged tree (`8c80bdf`, target dir clean): `cargo test --workspace` 148 passed / 0 failed,
  `cargo clippy --workspace --all-targets` clean, `cd dashboard && npx tsc --noEmit` clean. Playwright not run
  (dashboard untouched).
- Two failures seen while other worktrees ran `cargo test --workspace` concurrently were **not** this ticket:
  `cli.rs::rcli_agent_run_registers_and_connects_a_device` (Device shows `Online` before its `Hello` sets
  `agent_version` — a pre-existing race in `agent_link.rs`/`cli.rs`, both untouched by t/04) and
  `history.rs::rcli_metrics_history_lists_raw_samples_in_a_time_range` (a ~1 s clock-stamp jitter). Both passed in
  the clean run alone; leave them as separate flakes, not ticket 04 defects.
- Still not verified: real NAT behaviour, `X-Forwarded-For` behind a real reverse proxy (the shortcut above),
  and the `sign_in` name-spraying limit (ticket-unrelated, already noted in `auth.rs`).
