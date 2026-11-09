# 04: Brute-force protection for Device Passwords

**What to build:** The Server rate-limits wrong Device Passwords per (Device ID, IP): 5 failures lock out for 1 minute, each next lockout doubles, up to 1 hour. Every failure and lockout is audited. The CLI shows a clear message with the remaining lockout time. Must not be cut: it is the real protection behind a weak Server Key (ADR 0003). See `.scratch/easy-setup/spec.md` (Security).

**Blocked by:** 03

**Status:** ready-for-agent

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
- Verification: `crates/testkit/tests/rate_limit.rs` (8 tests, all through the Controller API or `rcli` in-process
  with `TestSystem::advance`): first lockout and remaining time, doubling to the hour cap, per-(Device, IP)
  isolation using the Server's `::1` side (`TestSystem::start_dual_stack`) for a genuinely different peer IP,
  counter reset on success, audit entries, the CLI message, wrong-ID indistinguishability, and parallel guesses.
  Full `cargo test --workspace` and `cargo clippy --workspace --all-targets` clean; the dashboard is untouched, so
  Playwright was not run.
- `shortcut:` left in `auth.rs`: the Server Key check is not counted (it reveals nothing about passwords, and a
  leaked key still has to pass the Device Password limiter); the failure window is fixed rather than sliding; a
  Server behind a reverse proxy sees the proxy's IP, which makes the limiter shared (and the naming wrong).
- Not verified: a real client behind NAT (all tests use loopback; isolation is proven with `127.0.0.1` vs `::1`).
