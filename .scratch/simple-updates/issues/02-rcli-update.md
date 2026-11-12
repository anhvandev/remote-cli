# 02: `rcli update`

**What to build:** `rcli update` moves the machine to the Latest Build for its target and only ever goes up. It prints the current version, the Latest Build's version, progress, the hash check and the restart. `rcli upgrade` stays as a hidden alias.

**Blocked by:** 01

**Status:** resolved

- [ ] Running below the Latest Build: downloads, verifies the sha256, swaps the binary, restarts the Agent service, and reports "updated X → Y"
- [ ] At the Latest Build: prints "already the latest version (X)" and exits 0 with no change
- [ ] Above the Latest Build: prints "running X, newer than the Server's latest (Y)" and exits 0 with no change
- [ ] No Agent Build for this target: a clear "no build for <target>" error
- [ ] `rcli upgrade` runs the same command and does not appear in help
