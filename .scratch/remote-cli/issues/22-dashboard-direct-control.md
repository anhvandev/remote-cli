# 22: Dashboard: direct control

**What to build:** A person in the Dashboard waits for the Control Lock like any Controller, acquires it, and then drives the Device with their own mouse and keyboard over the live view.

**Blocked by:** 21, 07

**Status:** ready-for-agent

- [ ] Acquire/release Control Lock from the Dashboard; holder and expiry shown when someone else holds it
- [ ] Mouse and keyboard input forwarded while holding the lock; refused otherwise
- [ ] Input audited like any other Action
- [ ] Playwright test covers waiting for and taking the lock
