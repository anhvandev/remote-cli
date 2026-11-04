# 12: Interactive shell from the CLI

**What to build:** `rcli shell <device>` opens an interactive PTY session that works like SSH, with the same run-as rules as ticket 11.

**Blocked by:** 11

**Status:** resolved

- [ ] Full PTY: resize, Ctrl-C, full-screen programs
- [ ] Same run-as and role rules as one-shot exec
- [ ] Session start/end audited

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
