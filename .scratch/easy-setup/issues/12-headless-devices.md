# 12: Devices without a display

**What to build:** An Agent with no display keeps metrics, exec, shell, power and Wake, and refuses shot, act, batch and Live View with a 'Device has no display' error. The Dashboard hides the Screen tab for such Devices and says why. The FakePlatform can be scripted with zero displays. See `.scratch/easy-setup/spec.md` (Headless Devices).

**Blocked by:** 01

**Status:** ready-for-agent

- [x] Screen Actions return the 'no display' error; everything else works
- [x] The Dashboard hides the Screen tab and explains why

## Comments

- Implemented on branch `t/12`. The Agent's Executor and Live refuse when `platform.displays()` is empty with
  `no_display` / "Device has no display"; the Server maps it to 409 `no_display` on `POST .../actions` and
  `POST .../live`. `crates/testkit/tests/headless.rs` covers shot, act, batch, Live View and that
  metrics/exec/shell/power/Wake still work on a zero-display FakePlatform. The Dashboard hides the Screen tab
  (and explains why) using `inventory.displays`; the devstack gained `juliet` (headless) and now fills inventory
  displays from the platform. `cargo test --workspace` 95 passed / 0 failed; clippy and `tsc --noEmit` clean.
  Playwright was not run in this worktree (no local `node_modules`/browser stack); the new
  `dashboard/tests/headless.spec.ts` is written but unverified.
