# 21: Windows adapter: logged-in session

**What to build:** Carried over from `.scratch/remote-cli/issues/28`. Platform port for Windows in a logged-in session. Capture and input reuse scrap/enigo (ADR 0002). Also covers the accessibility tree via UI Automation, inventory and resources, shell, power and Wake send. The user runs the contract suite on a real machine.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Platform contract suite passes on a real Windows 10/11 machine in a logged-in session
- [ ] Multi-display capture and input
- [ ] UI Automation tree feeds Marks

## Comments

- 2026-11-10: `WindowsPlatform` (`crates/agent/src/windows_platform.rs`) implemented on branch `t/21`; `rcli agent run` and `rcli-contract` use it on Windows. The Windows named-pipe IPC (`\\.\pipe\rcli-agent`) landed too, since `rcli` did not compile for Windows without it. Checked from macOS only: `cargo xwin clippy --target x86_64-pc-windows-msvc -p rcli -p rcli-agent --tests --features rcli/gui` is clean and `cargo xwin build` links `rcli.exe` and `rcli-contract.exe`. Nothing has run on Windows yet.

  **To run on a real Windows 10/11 machine** (logged in as a normal user, a normal non-elevated PowerShell; needs Rust and the MSVC Build Tools; the OCR check needs a Windows OCR language, which normally comes with the display language):

  ```powershell
  git clone <repo> remote-cli; cd remote-cli; git checkout t/21   # or easy-setup once merged
  cargo test -p rcli-agent windows_platform                       # key names, pointer coordinates, clipping, Wake packet, encoding
  notepad                                                         # an empty text field for the suite
  cargo run --release -p rcli-agent --bin rcli-contract
  ```

  Or cross-build on the Mac with `cargo xwin build --release --target x86_64-pc-windows-msvc -p rcli-agent --bin rcli-contract`, then copy `rcli-contract.exe` from `<target dir>/x86_64-pc-windows-msvc/release/` and run it.

  The suite prints its plan, then gives each step 20 s. Click into Notepad at "Click into an empty text field". At "Log out now", **do not log out**: a user-session process dies with the session; just wait. The Lock screen and Login screen columns, and `shell as root`, belong to ticket 22 (SYSTEM service) and are expected to fail here. This ticket passes when every **Logged in** cell is ✅ (`displays`, `capture`, `key shows on screen`, `session state`, `control indicator`, `text recognition`, `accessibility tree`, `metrics`, `inventory`, `processes`, `shell`, `shell timeout`, `pty`, `wake send`, `power lock`). Paste the table here. For multi-display, run it with two monitors attached (the `displays` check captures every display). Then try `rcli act <device> click` on the second monitor through a running `rcli agent run` and check that Marks come from the tree (`role` button/field/link, not just `text`).

  **Shortcuts taken (each marked `shortcut:` in the code):**
  - Capture uses crates.io `xcap` (GDI) and input uses crates.io `enigo`, not RustDesk's git `scrap`/`enigo` (ADR 0002). RustDesk's `scrap` needs vcpkg libvpx/aom/libyuv plus the `hbb_common` submodule, so it cannot be cross-built from the Mac. The pointer move is our own `SendInput` call, because enigo's absolute move only covers the primary display.
  - `shell` and `pty` run PowerShell as the account the Agent runs as, whatever `run_as` says. A user-session process cannot switch accounts; ticket 22's service must start them as the Terminal User or SYSTEM.
  - UI Automation walks only the foreground window, or the window at the display's centre when the foreground window is on another display.
  - There is no control indicator of its own: in the desktop build the GUI window is the indicator. Without the GUI, `ask_root` and `ask_accept` reject.
  - The inventory has no GPUs and no serial number (they need WMI). There is no GPU usage or CPU temperature. Battery comes from `GetSystemPowerStatus`.
  - IPC: an elevated caller counts as root. A user-session Agent cannot open an elevated caller's token, so there nobody counts as root until ticket 22's SYSTEM service.
  - `install_update` moves the running exe aside, writes the new one, starts it with the same arguments and exits. Ticket 22 should restart through the service manager instead.
  - `rcli-testkit` still does not compile for Windows (it uses the Unix-only `serve_trusting_all`). It is test infrastructure and runs on macOS/Linux only.
