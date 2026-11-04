# 03: Inventory and current resources

**What to build:** Operators can see each Device's inventory and current resources. Inventory is sent when the Agent starts and whenever it changes. Resources are sampled every 5 seconds and sent as deltas. The process list is fetched on demand only.

**Blocked by:** 01

**Status:** resolved

- [ ] Inventory: OS/version, hostname, CPU model/cores, RAM, disks, GPU, network interfaces (IP/MAC), serial, displays and resolutions
- [ ] Resources: CPU %, RAM used, disk usage %, disk read/write rate, network up/down, GPU %, uptime, logged-in user, screen-lock state; CPU temperature and battery when available
- [ ] `rcli device show <device>` returns inventory + latest resources (human and JSON)
- [ ] Inventory changes on the FakePlatform show up without a manual refresh
- [ ] `rcli device processes <device>` returns the process list on demand; it is not streamed periodically
- [ ] Online Device shows whether it is at the lock screen or login screen

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
