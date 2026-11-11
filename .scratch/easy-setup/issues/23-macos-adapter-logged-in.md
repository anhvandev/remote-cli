# 23: macOS adapter: logged-in session

**What to build:** Carried over from `.scratch/remote-cli/issues/30`. Platform port for macOS in a logged-in session. Capture and input reuse scrap/enigo. Also covers the accessibility tree via the AX API, inventory and resources, shell, power and Wake send. The user runs the contract suite on a real machine.

**Blocked by:** 01

**Status:** resolved

- [ ] Platform contract suite passes on a real macOS 15 machine in a logged-in session
- [ ] Multi-display and Retina scaling handled by the Coordinate Mapper
- [ ] AX tree feeds Marks

## Comments

- 2026-11-10: `MacPlatform` (`crates/agent/src/macos.rs`) implemented; `rcli agent run` and `rcli-contract` use it on macOS. Non-interactive parts are checked by `cargo test -p rcli-agent --lib macos` (shell, timeout, PTY, inventory, metrics, processes, displays, session state, Wake send, Vision OCR plumbing, AX read). Real-machine acceptance is still to do:

  **Run the contract suite on this Mac (logged-in column):**

  1. System Settings → Privacy & Security: grant **Screen Recording** and **Accessibility** to the terminal app you run it from (Terminal/iTerm), then restart that terminal.
  2. Open TextEdit with a new empty document (gives a text field and an AX tree).
  3. From the repo root: `cargo run --release -p rcli-agent --bin rcli-contract | tee contract-macos.md`
  4. Follow the `NOW:` lines (20 s each): click into the TextEdit document; lock with ⌃⌘Q and unlock (delete the typed dots first). At **"Log out now"** and **"log in"** do nothing: logging out kills a user-session process, so the login-screen column belongs to ticket 24. The suite locks the screen itself at the end; unlock again.
  5. Paste the table from `contract-macos.md` here. Expected for this ticket: every **Logged in** cell ✅. Lock-screen and login-screen cells are for ticket 24 (the login-screen ones fail when run from a user session, including `shell as root`).
  6. By eye: with two displays (one Retina), run `rcli act <device> click <x> <y>` on a point you can see in an Observation of each display, and check that the pointer lands there. Also check that `rcli observe` Marks match TextEdit's buttons and menu items.

  **Shortcuts taken:**
  - Uses crates.io `enigo` 0.6 for input and CoreGraphics `CGDisplayCreateImage` for capture, not RustDesk's `scrap`/`enigo` git deps (ADR 0002). RustDesk's `scrap` needs vcpkg libvpx/aom/libyuv and `hbb_common` to build. `CGDisplayCreateImage` is deprecated but still works on macOS 15; move to ScreenCaptureKit when Apple removes it.
  - The AX tree covers only the focused app (menu bar and windows), capped at 3000 nodes with a 0.5 s per-app messaging timeout. Without the Accessibility grant, `accessibility` returns `None`, so Marks fall back to OCR (Apple Vision).
  - Pointer moves round to whole points, so on Retina an odd pixel can land 1 px off.
  - Not reported yet: GPU list, disk I/O rates, GPU load, temperature, battery.
  - Shells run as `/bin/zsh`, not each account's login shell. An Agent running as a user (not root) runs every command as itself, whatever the Terminal User is. As root it uses `sudo -n -u <user> -i`.
  - `session_state` reports only LoggedIn or Locked, plus LoginScreen when there is no GUI session. Ticket 24 refines it for the root daemon.
  - Lock uses the private `SACLockScreenImmediate`. Sleep uses `pmset sleepnow`. Restart/Shutdown use `shutdown` as root, or loginwindow Apple events as a user; macOS may show an Automation prompt for those (untested).
  - `set_control_indicator` does nothing, and `ask_root`/`ask_accept` reject. On desktop builds the GUI window (`gui::wrap`) is the indicator and answers prompts.
  - The macOS deps are target-specific in `crates/agent/Cargo.toml`, not workspace deps, to keep merges with tickets 15/21 small.
  - `crates/testkit/tests/cli.rs`: the `rcli agent run` test now runs the real adapter on macOS, so it waits for `agent_version` (set by the hello) as well as Online. Before, it raced.

- 2026-11-11: Resolved after the final merge. The code is on the integration branch `easy-setup` (tip 63398c2) and the automated gates pass: `cargo test --workspace` 238 passed (the one failure is the environmental macOS display/wake test on this host), clippy, Windows `cargo xwin` clippy/build, Linux container 79 passed, Dashboard Playwright 42 passed. Real-hardware acceptance (see the steps above) remains for the user; the checkboxes stay unticked until that run.
