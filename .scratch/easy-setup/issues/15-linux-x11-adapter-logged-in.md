# 15: Linux X11 adapter: logged-in session

**What to build:** Carried over from `.scratch/remote-cli/issues/32`. Implement the Platform port for Linux X11 in a logged-in session. Capture and input reuse scrap/enigo (XTest). Also covers the accessibility tree via AT-SPI, inventory and resources, shell, power and Wake send. The control indicator is the GUI (ticket 14). The user runs the contract suite on a real machine, and the agent supplies the commands.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Platform contract suite passes on a real X11 desktop
- [ ] AT-SPI tree feeds Marks

## Comments

- Implemented `rcli_agent::linux::LinuxPlatform` (`crates/agent/src/linux/`), used by `rcli agent run` and
  `rcli-contract` on Linux; other OSes stay on the FakePlatform. Capture: RandR monitors + `GetImage` over x11rb.
  Input: enigo over XTest. AT-SPI: tree walk over the a11y D-Bus (zbus), mapped to button/field/link/... elements.
  OCR: tesseract CLI. Inventory/metrics/processes: sysinfo. Shell/PTY: `sh -c` / login shell on a PTY, own process
  group, `sudo -n` to switch accounts. Session state and power Lock: `loginctl ... auto` (`LockedHint`). Power:
  `systemctl reboot|poweroff|suspend`. Wake: UDP broadcast to port 9. Update: atomic swap then `exec` in place.
- Verified in Docker (Debian bookworm, Xvfb, dbus-run-session): `crates/agent/tests/linux.rs` 7/7 pass, including an
  end-to-end test that finds a zenity dialog's field and OK button through AT-SPI, clicks and types into it, and
  OCRs its label. macOS `cargo test --workspace` and clippy unchanged.
- Run on a real X11 desktop (logged in, from a terminal in the session, as your normal user):

  ```sh
  sudo apt install tesseract-ocr          # Fedora: sudo dnf install tesseract
  cargo test -p rcli-agent --test linux -- --include-ignored   # needs zenity too: sudo apt install zenity
  cargo run -p rcli-agent --bin rcli-contract | tee contract.md
  ```

  `rcli-contract` prints the steps, then gives 20 s per step (focus a text field, lock, unlock, log out, log in,
  unlock). Only the "Logged in" column is this ticket; the lock-screen and login-screen columns and "shell as root"
  belong to ticket 16 (root service) and are expected to fail when run as a user. Paste the table here.
- `shortcut:` plain `GetImage` (no MIT-SHM); OCR spawns tesseract each call; session state trusts logind
  `LockedHint` (xscreensaver and some lockers do not set it) and reads as logged in without a logind session;
  needs systemd >= 243 for the `auto` session name; no GPU %; a user Agent switches accounts only via sudoers;
  `set_control_indicator`/`ask_root`/`ask_accept` are no-ops/reject here because the GUI window (ticket 14) wraps
  this platform; the AT-SPI walk caps at 3000 nodes and sets `org.a11y.Status.IsEnabled` so Qt/Firefox/Chromium
  expose trees (apps started before may need a restart).
- ADR 0002 deviation: uses crates.io `enigo` + `x11rb` rather than RustDesk's `scrap`/`enigo` git deps (those need
  vcpkg-built libvpx/aom/yuv); same XTest approach, pure Rust, cross-builds cleanly.
- Contract suite change: `shell`/`pty` checks now run as the current account (`$USER`/`$USERNAME`) instead of a
  literal `user` account that real machines do not have.
