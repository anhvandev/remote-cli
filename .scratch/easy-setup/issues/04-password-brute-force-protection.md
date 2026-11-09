# 04: Brute-force protection for Device Passwords

**What to build:** The Server rate-limits wrong Device Passwords per (Device ID, IP): 5 failures lock out for 1 minute, each next lockout doubles, up to 1 hour. Every failure and lockout is audited. The CLI shows a clear message with the remaining lockout time. Must not be cut: it is the real protection behind a weak Server Key (ADR 0003). See `.scratch/easy-setup/spec.md` (Security).

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] Lockout after 5 failures, doubling up to 1 hour, verified with the injectable clock
- [ ] Lockouts are per (Device ID, IP): another IP or another Device is unaffected
- [ ] A correct password after the lockout ends works and resets the counter
- [ ] Failures and lockouts appear in the audit log
