# 07: Device actions on the Devices page

**What to build:** Each row on the Devices page gets a `⋯` menu with rename, change Device ID, change Device Password, merge, revoke and delete, the actions now on Admin › Device. Delete asks for confirmation, then removes the Device with its history and disconnects it. The Admin › Device page is removed.

**Blocked by:** 06

**Status:** resolved

- [x] Every action from Admin › Device works from the Devices page `⋯` menu
- [x] Delete asks for confirmation, then the Device disappears from the list
- [x] The Admin › Device page and its nav link are gone
- [x] The device-admin Playwright e2e covers the menu, including delete
