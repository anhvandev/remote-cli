# 18: Dashboard Connect guide page

**What to build:** A page for every signed-in User with: Server Address and Server Key with copy buttons; install commands per OS (curl, PowerShell) with the key filled in; download links for exe/dmg/deb/rpm; headless install; Controller setup (`rcli setup`, `rcli connect <ID>`); an MCP snippet for Claude Code; how to get past SmartScreen and Gatekeeper for unsigned builds; and a red warning when the Dashboard is served over http. See `.scratch/easy-setup/spec.md` (Dashboard).

**Blocked by:** 17, 10, 05

**Status:** ready-for-agent

- [ ] Every block is present, with working copy buttons
- [ ] The http warning shows only over http
- [ ] Playwright test covers the page
