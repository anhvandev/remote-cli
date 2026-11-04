# 07: Control Lock rules and on-Device indicator

**What to build:** Implement the full Control Lock rules from the glossary. It cannot be taken over. It ends on release, after 10 minutes without any Action from its holder (observe-only requests count), after 60 seconds of holder disconnection, or when an admin force-releases it. There is no queue. While a Controller holds the lock or a live stream is active, the Device shows a visible indicator.

**Blocked by:** 05, 06

**Status:** resolved

- [ ] Any Action from the holder, including observe-only requests, resets the 10-minute timer
- [ ] Lock survives a holder disconnect shorter than 60 s; released after 60 s
- [ ] Other Controllers (people included) cannot pre-empt the lock; the error carries holder and expiry time
- [ ] Admin force-release works and is audited
- [ ] Non-holders can still observe, read metrics and use the terminal
- [ ] Platform port shows/hides the control indicator; FakePlatform records it
- [ ] All timing verified with the injectable clock

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
