# 16: Linux: root service, install/uninstall, greeter, deb/rpm

**What to build:** Replaces `.scratch/remote-cli/issues/33`. `sudo rcli agent install --server … --key … [--allow-root]` copies the binary, records the Terminal User (`SUDO_USER`), registers the Device, and installs and starts a root systemd service. `sudo rcli agent uninstall` stops and removes the service, removes the Device on the Server and deletes local data. The service finds the seat0 greeter's X server and auth file and runs the helper as the greeter user, so the lock screen and display-manager login can be observed and controlled. The Secret Store uses `systemd-creds` (TPM when present). deb/rpm packages run the same install. Works on headless servers. See `.scratch/easy-setup/spec.md`.

**Blocked by:** 15, 06

**Status:** resolved

- [ ] Install, reboot (service comes back) and uninstall verified on a real desktop and a headless server
- [ ] Contract suite passes at the lock screen and at the GDM/LightDM/SDDM greeter
- [ ] `unlock` works at both
- [ ] deb/rpm install and remove cleanly

## Comments

- 2026-11-10: Implemented on `t/16`. What exists now:
  - `sudo rcli agent install --server … --key … [--name …] [--terminal-user …] [--allow-root]`
    (`crates/cli/src/linux_install.rs`): copies the running binary to `/usr/local/lib/rcli/rcli`, links
    `/usr/local/bin/rcli`, records Device Settings (Terminal User = `--terminal-user`, else `SUDO_USER`; `--allow-root`
    = root always), registers the Device **before** enabling anything (a wrong Server Key fails at install and the
    Device ID + Device Password are printed), then writes and starts `rcli-agent.service`
    (`ExecStart=… agent run --server … --state-dir /var/lib/rcli --name … --session-socket /run/rcli/session.sock`,
    `Restart=always`, `WantedBy=multi-user.target`, `StateDirectoryMode=0700`). The Server Key is in
    `/etc/rcli/agent.env` mode `0600` (`EnvironmentFile=`), never on a command line. Every path hangs off `Layout.root`
    and every command goes through the `Run` seam, so layout/unit/pipeline are unit-tested without root.
  - `sudo rcli agent uninstall`: asks the Server to delete this Device (new protocol message, audited as an Admin
    `device.delete` with `self: true`), then stops/disables the unit, removes the unit, key file, binary, link, session
    socket and `/var/lib/rcli` (credentials, settings, secrets). An unreachable Server is a warning, not a failure.
    macOS uninstall now makes the same self-removal call; installs write `install.json` (Server Address only) so
    uninstall can reach the Server before the credentials are gone. The old `shortcut:` comment about no self-removal
    message is gone.
  - Console session (`crates/agent/src/linux/session.rs`): the root service finds seat0's X11 session through
    `loginctl show-seat seat0` / `show-session` (user, UID, class, type, display, `LockedHint`), the X server's own
    `-auth` argument in `/proc/<pid>/cmdline`, `/run/lightdm/root/:N`, `/run/gdm{,3}/authority`,
    `/run/sddm/xauth*`, `/var/lib/gdm{,3}/.Xauthority`, `/run/user/<uid>/gdm/Xauthority` and `~/.Xauthority`, then
    runs `rcli agent session` through `runuser -u <user> -- env DISPLAY=… XAUTHORITY=…` (one helper for the console,
    killed and restarted on login/logout with a 2 s reconciler). It reuses the shared helper wire
    (`crates/agent/src/session.rs`, moved out of `macos_session.rs`) to forward displays, capture, input,
    accessibility and session state. Shell, PTY, power, metrics, processes and inventory stay in the daemon. A Wayland
    session or greeter is refused with "no X11 session on the console" instead of showing a useless Xwayland fragment.
    Deliberate design choice: a root daemon *could* read the cookie and talk to X directly, but enigo input needs the
    process-global `DISPLAY`/`XAUTHORITY` and those change with the console session; the helper (already needed for
    macOS) is the simplest way to keep that environment per-session.
  - Secret Store (`crates/agent/src/secret_store.rs`): `SystemdCredsSecretStore` for the root Agent on Linux —
    `systemd-creds encrypt --name <name> --with-key auto` (host key + TPM when present; the name is bound into the
    key, system-scope credentials are root-only) and `decrypt`; `systemd-creds` is detected at runtime, and
    `FileSecretStore` remains for non-root/test/without-systemd. Install prints a warning when it falls back.
  - Packaging (`packaging/linux/`): `build-deb.sh` + `build-rpm.sh` build a package that installs only
    `/usr/bin/rcli`; `postinst` reads `/etc/rcli/agent.conf` (`RCLI_SERVER`, `RCLI_SERVER_KEY`, optional
    `RCLI_TERMINAL_USER`/`RCLI_ALLOW_ROOT`) and calls the same `rcli agent install`; `prerm` calls
    `rcli agent uninstall`; `postrm purge` removes the config. `docs/deploy.md` §10 has build, install, reboot,
    greeter, lock screen, unlock, deb/rpm, upgrade, uninstall, Wayland and headless steps with expected output.
- Verified here (macOS host; Linux in a Debian bookworm ARM64 container with `CARGO_TARGET_DIR=/tmp/target`):
  - `cargo test --workspace` (with `DOCKER_HOST=unix:///Users/vannabeki/.colima/default/docker.sock`): all test
    binaries pass, 0 failed.
  - `cargo clippy --workspace --all-targets -- -D warnings`: clean.
  - Linux: `cargo clippy -p rcli-agent -p rcli --all-targets -- -D warnings`: clean;
    `cargo test -p rcli --lib linux_install`: 4/4; `cargo test -p rcli-agent --lib linux::`: 7/7 (greeter/user
    session parsing, target/auth discovery against a fake root + `/proc`, helper command line);
    `xvfb-run -a cargo test -p rcli-agent --test linux -- --include-ignored`: the new
    `the_session_helper_forwards_screen_calls_to_the_x_server` passes (real Linux X11 adapter behind the helper wire
    on Xvfb); 7/8 total, the pre-existing AT-SPI/zenity test needs `dbus-run-session`, zenity, tesseract and fails in
    the bare image (documented in the test file header, not affected by this ticket).
  - `cargo test -p rcli-testkit --test unregister`: passes (self-removal deletes the Device, audits it, keeps other
    Devices, closes the connection).
  - Windows: `cargo xwin clippy --target x86_64-pc-windows-msvc -p rcli-agent -p rcli -p rcli-server --all-targets -- -D warnings`:
    clean. The full `--workspace` xwin run still fails on a pre-existing `#[cfg(unix)] serve_trusting_all` use in
    `crates/testkit` (present on the `easy-setup` tip as well), not from this ticket.
  - `packaging/linux/build-deb.sh` built `rcli_0.1.0_arm64.deb` in the container; `dpkg-deb -I` shows only
    `/usr/bin/rcli` plus the three maintainer scripts; `dpkg-deb -c` shows the payload. The RPM script was syntax-checked
    only (no `rpm-build` in the image).
  - Nothing was installed on this Mac; no macOS/Linux service was touched.

**Real-machine acceptance (Linux, X11, root). Run on a test machine; paste results here.**

0. Prerequisites: a desktop or server with systemd; for screen work an Xorg session and, ideally,
   `sudo apt install tesseract-ocr xauth`. A Wayland session/greeter is out of scope (see §10.3 of docs/deploy.md).
1. Build and install:
   ```sh
   cargo build --release -p rcli
   sudo install -m 755 target/release/rcli /usr/local/bin/rcli
   sudo rcli agent install --server vps.example.com:8443 --key rk_<fingerprint>_<secret>   # --allow-root if wanted
   ```
   Expected: `installed rcli-agent.service and /usr/local/lib/rcli/rcli`, `Device ID: 9 digits`,
   `Device Password: …`. Wrong key (change one hex digit): the command fails with `invalid_server_key` and
   `systemctl status rcli-agent.service` says `could not be found` (the service is not enabled).
2. Check the service and secrets:
   ```sh
   systemctl is-enabled rcli-agent.service   # enabled
   systemctl is-active rcli-agent.service    # active
   rcli agent info                            # connected, Device ID
   sudo rcli agent password show              # the printed password
   sudo stat -c '%a %U' /etc/rcli/agent.env   # 600 root
   systemctl cat rcli-agent.service | grep -E 'ExecStart|EnvironmentFile'   # no rk_ in ExecStart
   ps -ef | grep '[r]cli agent run'           # no rk_ argument
   ```
3. Reboot: `sudo reboot`; after login/at the greeter `systemctl is-enabled …` = enabled,
   `systemctl is-active …` = active, `rcli agent info` = connected, Dashboard shows the Device Online.
4. Logged-in session (from a Controller): `rcli shot <id> --out /tmp/s` shows the desktop; `rcli act <id> type hello`
   and `rcli act <id> key enter` work; `rcli exec <id> -- whoami` prints the Terminal User (the account that ran
   `sudo`, e.g. `alice`); `rcli shell <id>` opens a shell; `rcli metrics <id> history` has samples.
5. Lock screen: `rcli power <id> lock` (or `loginctl lock-session` on the machine). Expected: `rcli device show <id>`
   shows `"session": "LockScreen"`; `rcli shot <id>` is the lock screen; `rcli act <id> key a` shows a dot; after
   `sudo rcli agent unlock-password set` on the machine, `rcli unlock <id>` returns to the desktop. Repeat a lock a
   second time to confirm it is not a one-off.
6. Greeter: log out of the desktop (GDM/LightDM/SDDM). Expected: `rcli device show <id>` → `"session": "LoginScreen"`;
   `rcli shot <id>` shows the greeter; `rcli act <id> type <password>` then `rcli act <id> key enter` logs in (or
   `rcli unlock <id>` types the stored unlock password); after login the screenshot shows the desktop again and the
   session state becomes `Active`. Do this once per display manager you can install (GDM, LightDM, SDDM).
7. Contract suite at the lock screen and greeter (root, on the machine):
   ```sh
   cargo build --release -p rcli-agent --bin rcli-contract
   sudo ./target/release/rcli-contract --session-socket /run/rcli/session.sock | tee contract-linux.md
   ```
   Follow the `NOW:` lines at the machine (20 s each: focus a text field, lock, unlock, log out, log in, unlock).
   Expected: `Logged in` all ✅; `Lock screen` all ✅; `Login screen` capture/key/session state ✅ (XTest works at the
   greeter on Linux, unlike macOS) and `shell as root` ✅; `control indicator` is a no-op (the GUI window of ticket 14
   is the indicator) and `text recognition` needs tesseract. Paste the markdown table here.
8. deb (on Debian/Ubuntu; `sudo apt install dpkg-dev`):
   ```sh
   packaging/linux/build-deb.sh                         # → packaging/linux/out/rcli_0.1.0_<arch>.deb
   sudo install -m 600 /dev/null /etc/rcli/agent.conf
   sudo tee /etc/rcli/agent.conf >/dev/null <<'CONF'
   RCLI_SERVER=vps.example.com:8443
   RCLI_SERVER_KEY=rk_<fingerprint>_<secret>
   CONF
   sudo apt install ./packaging/linux/out/rcli_0.1.0_<arch>.deb
   ```
   Expected: `systemctl is-active rcli-agent.service` = active, `dpkg -s rcli` = installed, `dpkg -L rcli` lists
   `/usr/bin/rcli`, `rcli agent info` connected. `sudo apt remove rcli`: the Device disappears from `rcli devices`,
   the unit and `/var/lib/rcli` are gone, `/etc/rcli/agent.conf` is kept; `sudo apt purge rcli` also removes the config.
   RPM: same with `packaging/linux/build-rpm.sh` and `sudo dnf install ./packaging/linux/out/rcli-0.1.0-1.<arch>.rpm`.
9. Headless server (no DISPLAY): same install command. Expected: service active and enabled after reboot;
   `rcli metrics/exec/shell/power` work; `rcli shot <id>` returns
   `no X11 session on the console: cannot see the screen`; Dashboard hides Screen.
10. Upgrade: either `sudo rcli upgrade` (server rollout, signed) or install the newer deb/rpm. Expected: Device ID and
    password unchanged, service active, `rcli agent info` connected, Dashboard shows the new version.
11. Uninstall: `sudo rcli agent uninstall`. Expected: `rcli devices` no longer lists the Device (audit has
    `device.delete` with `self: true`), `systemctl status rcli-agent.service` = not found, and
    `/usr/local/lib/rcli/rcli`, `/usr/local/bin/rcli`, `/etc/systemd/system/rcli-agent.service`,
    `/etc/rcli/agent.env`, `/var/lib/rcli` are gone (the package's `/usr/bin/rcli` stays if it came from deb/rpm).

**Shortcuts taken:**
- Greeter/session discovery is heuristic: logind seat0 first, then the Xorg `-auth` argument, then well-known
  display-manager paths and the user's home. It was unit-tested against a fake root/`/proc`; only real GDM/LightDM/SDDM
  machines can confirm the paths. Only X11: Wayland (including a Wayland GDM greeter) is refused with a clear error;
  forcing the greeter to Xorg is documented in `docs/deploy.md` §10.3. Multi-seat and fast user switching are not
  tested; the 2 s reconciler follows the active seat0 session.
- The helper is started with `runuser` from util-linux; there is no setuid fallback if `runuser` is missing.
- `systemd-creds`' real encryption (host key, TPM) could not be exercised on macOS. The store's arguments are unit
  tested with a fake runner; the fallback to `FileSecretStore` is detected at runtime and surfaced by install.
  Binding is the credential name + host (+ TPM) and system scope's root-only access; no per-UID binding is added.
- The deb was built and inspected in a Debian container; neither deb nor rpm was installed on a real distro. The
  postinst/prerm/postrm scripts are syntax-checked shell (`sh -n`) but only a real `dpkg`/`dnf` run proves them.
- `rcli agent install` leaves the copied binary/settings behind if registration fails (the unit is not written/enabled);
  rerunning install is safe. A successful install reuses existing `credentials.json`.
- The service is not sandboxed (`User=root`, state dir 0700): it must drive X, run commands as any Terminal User,
  manage power and replace its own binary (`ProtectSystem` would break `rcli upgrade`).
- Logs go to journald only (`journalctl -u rcli-agent.service`); no log file is written.
- `install.json` in the state dir stores only the Server Address for uninstall; the Server Key stays in the 0600
  `agent.env` (or the macOS daemon plist).
- macOS installs made before this change have no `install.json`, so their uninstall skips the Server removal (fresh
  installs and reinstalls write it); ticket 24's comment about manual `rcli device delete` is stale for fresh installs.
- The package owns only `/usr/bin/rcli`; the install path owns `/usr/local/lib/rcli/rcli` and `/usr/local/bin/rcli`.
  Removing the package with `--force-all` without its `prerm` would leave those behind.

- final code-review fix (P2, S4): `rcli agent install` now also writes the app-menu entry
  `/usr/share/applications/rcli.desktop` (source `packaging/linux/rcli.desktop`; uninstall removes it), so the window
  opens from the app menu as the spec says. The deb/rpm postinst calls the same install, so the package path gets it
  too without dpkg/rpm owning the file. The installer shares the new `crates/cli/src/install.rs` helpers
  (`Options`/`Run`/`write`/`at`/`save_settings`) with macOS and Windows instead of its own copies. Human step on a
  real desktop: after install, "rcli" appears in the app menu and opens the window; the desktop-entry install/removal
  is covered by the Linux container test (`cargo test -p rcli --lib linux_install`).

- 2026-11-11: Resolved after the final merge. The code is on the integration branch `easy-setup` (tip 63398c2) and the automated gates pass: `cargo test --workspace` 238 passed (the one failure is the environmental macOS display/wake test on this host), clippy, Windows `cargo xwin` clippy/build, Linux container 79 passed, Dashboard Playwright 42 passed. Real-hardware acceptance (see the steps above) remains for the user; the checkboxes stay unticked until that run.

- code-review fix (round 4): the deb and rpm are built with `--features gui` (eframe loads X11/Wayland with dlopen,
  so a headless machine still runs the binary), and the root service opens the prompt window with the console
  session's DISPLAY/XAUTHORITY or WAYLAND_DISPLAY/session bus instead of a bare `sudo -u`. Without both, `ask` and
  `password+accept` refused after 30 s and the audit said `rejected`.
