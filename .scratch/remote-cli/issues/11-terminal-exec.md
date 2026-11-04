# 11: One-shot terminal command

**What to build:** A Controller runs a command on a Device and gets stdout, stderr and the exit code, with a timeout. Commands run as the logged-in user by default. `--as root` (SYSTEM on Windows) is admin-only. When nobody is logged in, only admins can run commands, as root/SYSTEM.

**Blocked by:** 02, 06

**Status:** resolved

- [x] `rcli exec <device> -- <cmd>` returns stdout, stderr, exit code; timeout is enforced
- [x] Default run-as is the logged-in user
- [x] `--as root` refused for operators
- [x] With no logged-in user, operators are refused; admins run as root/SYSTEM
- [x] Command and exit code are audited
- [x] Does not require the Control Lock

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
