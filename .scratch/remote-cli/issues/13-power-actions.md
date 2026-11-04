# 13: Power Actions

**What to build:** Operators can Restart, Shut down, Sleep or Lock the screen of a Device. When someone holds the Control Lock, Restart, Shut down and Sleep are allowed only to the holder or an admin.

**Blocked by:** 07

**Status:** ready-for-agent

- [ ] `rcli power <device> restart|shutdown|sleep|lock`
- [ ] Restart/Shutdown/Sleep refused to non-holders (except admins) while a Control Lock is held
- [ ] Lock screen needs no Control Lock
- [ ] All power Actions audited
