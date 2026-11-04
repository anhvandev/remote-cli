# 27: Platform contract test suite

**What to build:** Build one contract test suite for the Platform port that runs against any adapter on a real machine. It checks capture, key injection showing up on screen, session-state detection, metrics, shell, power and Wake send, logged in and at the lock and login screens, with an operator following prompts like the lockscreen probe. First run it against the FakePlatform.

**Blocked by:** 01

**Status:** resolved

- [ ] Suite runs against the FakePlatform in CI
- [ ] Suite can run on a real machine with step-by-step operator prompts (lock now, log out now)
- [ ] Produces a results table like the lockscreen probe's README

## Comments

- 2026-11-04: Resolved on integration branch `feat/remote-cli` (tip after review fixes: 793f229). Code review findings fixed on branch `review-fixes`.
