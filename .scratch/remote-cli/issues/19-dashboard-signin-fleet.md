# 19: Dashboard: TOTP sign-in, fleet list, Device detail

**What to build:** Stand up the Next.js Dashboard. People sign in with password + TOTP and see the fleet with Device Status, plus each Device's inventory and current resources, lock/login-screen state and Control Lock holder. Add it to Docker Compose.

**Blocked by:** 02, 03

**Status:** ready-for-agent

- [ ] Password + TOTP sign-in; TOTP enrollment for a user
- [ ] Fleet list with Device Status (Online/Offline/Waking)
- [ ] Device detail with inventory, current resources, lock/login-screen state, Control Lock holder and expiry
- [ ] Dashboard is in the Docker Compose stack
- [ ] Playwright test: sign in and open a Device
