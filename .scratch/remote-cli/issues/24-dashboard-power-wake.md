# 24: Dashboard: power and Wake buttons

**What to build:** Each Device has buttons for Wake, Restart, Shut down, Sleep and Lock screen, with the same rules and error messages as the CLI.

**Blocked by:** 19, 13, 14

**Status:** resolved

- [ ] Buttons trigger the same Controller API operations
- [ ] Waking state and "Wake failed" reason shown
- [ ] Refusals (Control Lock held, no Wake Relay) shown clearly

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
