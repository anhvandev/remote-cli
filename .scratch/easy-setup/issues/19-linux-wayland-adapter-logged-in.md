# 19: Linux Wayland adapter: logged-in session

**What to build:** Carried over from `.scratch/remote-cli/issues/34`. Platform port for Wayland sessions (GNOME, KDE, wlroots). Capture uses the ScreenCast portal + PipeWire with a persisted restore token, which needs one approval at install. Input goes through uinput from the root service. The user runs the contract suite on real machines.

**Blocked by:** 16

**Status:** resolved

- [ ] Contract suite passes logged in on GNOME and KDE; wlroots (Sway/Hyprland) result recorded
- [ ] The restore token persists across Agent restarts, so no repeated approval is needed
- [ ] AT-SPI tree feeds Marks where available, OCR otherwise

## Comments

- 2026-11-11: Implemented on `t/19`. What exists now:
  - **Adapter selection** (`crates/agent/src/linux/mod.rs`): `SessionKind::detect` picks Wayland for
    `XDG_SESSION_TYPE=wayland` (an X11 session can carry a `DISPLAY` for Xwayland) or for no `DISPLAY` but a
    `WAYLAND_DISPLAY`; an explicit non-Wayland type wins over a stray `WAYLAND_DISPLAY` (a nested compositor). One
    `LinuxPlatform` keeps the `Platform` shape and sends displays/capture/input to `x11` or `wayland`; AT-SPI, OCR,
    metrics, inventory, shell, PTY, power, updates and prompts stay shared. A dev `rcli agent run` in a Wayland
    session holds the portal itself (`LinuxPlatform::with_state_dir`); inventory never opens the portal (it reports
    displays only once it is already open), so no dialog appears before a Controller asks for the screen.
  - **Capture** (`crates/agent/src/linux/screencast.rs`): `org.freedesktop.portal.ScreenCast` through ashpd 0.14 —
    `CreateSession`, `SelectSources` (MONITOR, multiple, cursor embedded, persistent, the stored restore token),
    `Start` (one stream per approved monitor), `OpenPipeWireRemote`. A dedicated thread consumes the streams with the
    `pipewire` crate (`AUTOCONNECT | MAP_BUFFERS`), parses `VideoInfoRaw` and converts packed
    RGBA/RGBx/BGRx/BGRA/RGB/BGR buffers to `Frame` (RGBA) honouring the chunk offset and stride; YUV layouts are
    declined with a clear error instead of shown garbled. `displays()` reports the stream's real pixels and scales the
    portal's logical positions (fractional scaling); `capture(i)` is stream i.
  - **Restore token** (`TokenStore`): `<state dir>/screencast.restore_token` (0600, temp file + rename). The root
    service passes it to the helper as `RCLI_SCREENCAST_RESTORE_TOKEN`; a helper's new token comes back in the session
    wire's state reply (`crates/agent/src/session.rs`) and the daemon writes it, skipping unchanged values. One
    approval at the Device therefore survives Agent, helper and service restarts.
  - **Console session** (`crates/agent/src/linux/session.rs`): ticket 16's Wayland refusal is gone. The reconciler
    targets the user's `wayland-N` socket under `/run/user/<uid>` and spawns the helper with `WAYLAND_DISPLAY`,
    `XDG_RUNTIME_DIR`, `XDG_SESSION_TYPE=wayland` and `DBUS_SESSION_BUS_ADDRESS`. Wayland input is written by the root
    service through `uinput` (`crates/agent/src/linux/uinput.rs`), not the helper: absolute pointer across the desktop
    bounding box (`ABS_X`/`ABS_Y` `0..=32767`, `INPUT_PROP_POINTER`), buttons, `REL_WHEEL`/`REL_HWHEEL`, evdev keys,
    US-layout text with Shift. A Wayland GDM greeter has no portal, so it is still "unavailable" — with its own
    message, and uinput input does reach it.
  - **Accessibility**: the shared AT-SPI walk (`atspi.rs`) is compositor-agnostic and unchanged; without a tree the
    Agent falls back to tesseract as before. Session state/power/lock are ticket 16's logind path.
  - **Contract suite**: a new `text input` row in every environment (the Wayland keyboard's keymap/Shift path is not
    exercised by one key); `rcli-contract` gained `--state-dir` for the portal token outside the service.
  - `docs/deploy.md` §10/§10.3 now document the Wayland path instead of refusing it.
- Verified here (macOS host; Linux in the container image `rcli-linux-wayland-dev`: `rust:1-bookworm` plus
  `clang libclang-dev pkg-config libpipewire-0.3-dev libspa-0.2-dev`, native aarch64):
  - Linux: `cargo clippy -p rcli-agent -p rcli --all-targets -- -D warnings` clean;
    `cargo test -p rcli-agent -p rcli`: 49 unit tests (28 in `linux::`), 3 contract, 5 in `tests/linux.rs` pass,
    3 ignored (need Xvfb/AT-SPI/tesseract).
  - macOS: `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` passes except
    `macos::tests::displays_session_wake_and_text_recognition_work_without_grants`, which fails for an environment
    reason: a bare `CGGetActiveDisplayList` returns 0 active displays in this session (the Mac's display is asleep),
    unrelated to this branch.
  - Windows: `cargo xwin clippy --target x86_64-pc-windows-msvc -p rcli -p rcli-agent --all-targets -- -D warnings`
    clean.
  - `easy-setup` (t/22) merged; conflicts in `session.rs`, `linux/session.rs` and `cli/agent.rs` resolved by keeping
    t/22's transport-neutral `Helper`/`allow_login_input` and layering the token sink plus
    `with_state_dir`/`console(state_dir)` on top; the new `Input::Sas` is Linux-unavailable like X11.
- Nothing was installed on real hardware and no portal dialog was opened here.

**Real-machine acceptance (Linux Wayland). Run on a test machine; paste results here.**

0. Prerequisites: a Wayland desktop with a ScreenCast portal (`xdg-desktop-portal` + `-gnome`/`-kde`/`-wlr`), and
   `/dev/uinput` (`sudo modprobe uinput`, plus `uinput` in `/etc/modules-load.d/uinput.conf`), `tesseract-ocr` for OCR.
   Build and install as in ticket 16:
   ```sh
   cargo build --release -p rcli
   sudo install -m 755 target/release/rcli /usr/local/bin/rcli
   sudo rcli agent install --server vps.example.com:8443 --key rk_<fingerprint>_<secret>
   ```
1. One approval, then never again (from a Controller):
   ```sh
   rcli shot <id> --out /tmp/wayland-1.png        # expected: a dialog appears at the Device: pick monitor(s), Share
   sudo cat /var/lib/rcli/screencast.restore_token && sudo stat -c '%a %U' /var/lib/rcli/screencast.restore_token
   # expected: a token, and "600 root"
   sudo systemctl restart rcli-agent.service
   rcli shot <id> --out /tmp/wayland-2.png        # expected: NO dialog, the desktop again
   ```
2. Input (pointer + keyboard + text):
   ```sh
   rcli act <id> move 960 540                     # expected: the pointer jumps there (absolute)
   rcli act <id> click                            # expected: what is under the pointer activates
   rcli act <id> type hello                       # expected: "hello" in the focused field
   rcli act <id> key ctrl+shift+t                 # expected: the shortcut fires
   rcli act <id> scroll --dy 3                    # expected: the page scrolls down
   ```
3. Marks and OCR: `rcli shot <id> --marks` → expected: buttons/fields/links boxed from AT-SPI; hide the trees (or
   use an app exposing none, e.g. a game) and expect tesseract text boxes instead. `rcli act <id> click --text "OK"`.
4. Session state/power: `rcli device show <id>` (session `LoggedIn`), `rcli power <id> lock` → session `Locked` and
   `rcli shot` shows the lock screen, `rcli unlock <id>` (with `sudo rcli agent unlock-password set`) returns to the
   desktop.
5. Contract suite (root, on the machine; keep the installed token):
   ```sh
   cargo build --release -p rcli-agent --bin rcli-contract
   sudo ./target/release/rcli-contract --session-socket /run/rcli/session.sock --state-dir /var/lib/rcli \
        | tee contract-wayland-gnome.md
   ```
   Expected: `Logged in` all ✅; `Lock screen` ✅ (if the compositor pauses the stream while locked, capture/text show
   ❌ "sent no frame" — record it); `Login screen` capture/text ❌ "screen capture at a Wayland login screen is not
   available" (expected: GDM's greeter has no portal), `shell as root` ✅, `session state` ✅.
   Also run the logged-in column without the service (portal approval on first run):
   ```sh
   ./target/release/rcli-contract --state-dir ~/.local/state/rcli-contract   # in the desktop session, as the user
   ```
6. Repeat on **KDE Plasma Wayland** (portal `xdg-desktop-portal-kde`) and one **wlroots** compositor (Sway or
   Hyprland with `xdg-desktop-portal-wlr` running in the session). Record per compositor: whether the dialog was
   asked once or every session (wlroots may not return a restore token), whether `move` was absolute, whether the
   lock screen kept streaming, and the greeter result.

**Shortcuts taken:**
- Capture is live-only: one portal session per helper/Agent process, opened on the first capture. A compositor that
  revokes the session (logout, portal restart) is handled by replacing the helper on login/logout; a dev Agent needs a
  restart. No reconnect/back-off inside `screencast`.
- `capture` returns the newest frame the compositor sent. If a compositor pauses the stream at the lock screen, the
  frame can predate the lock (the contract's lock-screen row records GNOME/KDE/wlroots behaviour).
- Only monitors the person selects are exposed; the portal's `restore_data` triple is ignored and only the plain
  restore token is stored (some wlroots portals return none → approval per session).
- YUV stream formats are declined with a clear error rather than converted; mutter/kwin/wlroots offer packed RGB.
- Text input is US-layout through evdev codes (the compositor's layout applies); non-ASCII characters are refused
  with "cannot type '<c>' …". Named keys follow the current layout.
- Wayland input needs `/dev/uinput` (root); absolute moves assume the compositor treats the `INPUT_PROP_POINTER`
  device's ABS axes as an absolute pointer. If a compositor ignores it, keys/buttons still work and moves do nothing.
- The portal approval happens at the first capture, not literally inside `rcli agent install`: `sudo` drops the
  session bus environment, and the portal call must run as the session user. It is still one-time.
- AT-SPI extents on a fractional-scaled Wayland desktop are logical pixels while frames are real pixels; Marks may be
  offset at scale > 1 (not scaled here).
- A Wayland GDM greeter remains uncapturable; the runbook keeps the `WaylandEnable=false` workaround for the login
  screen.
- `capture`/`displays` wait 10 s for the first frame of each stream and 5 s for a later frame; the portal request
  itself has 60 s for the person to approve.

- 2026-11-11: Resolved after the final merge. The code is on the integration branch `easy-setup` (tip 63398c2) and the automated gates pass: `cargo test --workspace` 238 passed (the one failure is the environmental macOS display/wake test on this host), clippy, Windows `cargo xwin` clippy/build, Linux container 79 passed, Dashboard Playwright 42 passed. Real-hardware acceptance (see the steps above) remains for the user; the checkboxes stay unticked until that run.
