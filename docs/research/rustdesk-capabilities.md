# RustDesk capabilities: login, lock, and secure-desktop access, plus capture and input code

Source: https://github.com/rustdesk/rustdesk, shallow clone at commit `55786aaa5709ac0a450aabcf06f9d1fee3119908` (committed 2026-11-03).
The `libs/hbb_common` submodule pointer is `fae47e024dd83cef6aaecde0aa7d11731df11649`. It was not cloned.
All paths below are relative to the repo root. Nothing was built or run.
Claims marked **UNVERIFIED** were not confirmed by the code or the docs.

## 1. Windows: login screen, lock screen, and UAC secure desktop

- **A service runs as SYSTEM.** The installer runs `sc create <app> binpath= "<exe> --service" start= auto` (`src/platform/windows.rs:4074-4091`). There is no `obj=` argument, so the service gets the `sc` default account, LocalSystem. `--service` calls `start_os_service()`, which calls `service_dispatcher::start` (`src/core_main.rs:381-384`, `src/platform/windows.rs:551-557`).
- **The service starts a helper in the active console session that runs with winlogon's token.** These are the steps:
  - `run_service` reads the active session with `WTSGetActiveConsoleSessionId` (or the RDP session) through `get_current_session` (`src/platform/windows.cc:582-584`).
  - It then calls `launch_server(session_id)`, which spawns `"<exe>" --server` (`src/platform/windows.rs:694-696, 814-823`).
  - `LaunchProcessWin(..., as_user=FALSE)` finds `winlogon.exe` in the target session (`windows.cc:143-167`) and opens its token with `OpenProcessToken` (`windows.cc:222-238`).
  - Finally it calls `CreateProcessAsUserW` (`windows.cc:250-290`).
  - Result: the `--server` process runs as SYSTEM inside the user's session.
- **The service follows session changes.** The service loop restarts `--server` when the active session changes or the helper exits (`windows.rs:701-790`).
- **Desktop switching (Default, Winlogon, or the UAC secure desktop):**
  - `inputDesktopSelected()` compares the thread's desktop with `OpenInputDesktop(...)` (`windows.cc:313-344`). `selectInputDesktop()` calls `SetThreadDesktop` on the input desktop (`windows.cc:347-365`, `switchToDesktop` at `windows.cc:294-308`). This code is credited to TigerVNC (`windows.cc:310`).
  - Input: `try_change_desktop()` (`windows.rs:1034-1045`) runs before injection in `input_service.rs:842, 1289, 1725, 2210, 2255`. Injection goes through enigo's `SendInput` (`libs/enigo/src/win/win_impl.rs:39, 87`).
  - Capture: the video loop bails with `"Desktop changed"` so the capturer is rebuilt on the new desktop (`src/server/video_service.rs:798-804`). The capturer is DXGI with a GDI fallback (`libs/scrap/src/dxgi/mod.rs`, `libs/scrap/src/dxgi/gdi.rs`).
- **Ctrl+Alt+Del:** the `--server` helper sends `ipc::Data::SAS` to the service, which calls `SendSAS(FALSE)` (`windows.rs:735-736, 941-1020`). It sets the `SoftwareSASGeneration` registry value temporarily and restores it afterwards. SAS is enabled only when running as root (`src/server/connection.rs:2080-2083`).
- **State detection:** `is_prelogin` returns true when the session user is empty or SYSTEM (`windows.rs:1255-1260`). `is_locked` uses the WTS session flags (`windows.rs:1262-1267, 1306-1318`). `is_logon_ui` looks for `LogonUI.exe` (`windows.rs:1269-1278`).
- **Portable (not installed) mode:** with `--elevate`, the app re-launches itself as SYSTEM with `--run-as-system` through `impersonate_system::run_as_system`. This "portable service" exchanges frames over shared memory (`windows.rs:2462-2530`, `src/server/portable_service.rs:1072-1090, 697-698`). Without elevation, the UI string says: "Due to UAC, RustDesk can not work properly as the remote side in some cases" (`src/lang/en.rs:36`). The string `elevated_foreground_window_tip` says it cannot control elevated windows (`en.rs:119`).

## 2. macOS: login window and lock screen

- **Supported through a LaunchAgent with `LimitLoadToSessionType = [LoginWindow, Aqua]`.**
  - The agent runs `RustDesk --server` with `RunAtLoad` and `KeepAlive`, `ProcessType=Interactive` (`src/platform/privileges_scripts/agent.plist:11-15, 27-31`). It is installed to `/Library/LaunchAgents/` (`src/platform/macos.rs:226-238`).
  - The install script loads it with `launchctl load -w -S LoginWindow` when no user is logged in, because "At the login window there is no gui/0 domain" (`macos.rs:1372-1379`).
- **A LaunchDaemon runs the `service` binary as root** (`privileges_scripts/daemon.plist:15-20`; the binary is defined in `Cargo.toml:19-21` and `src/service.rs:6-19`). It starts the `_service` IPC listener and the auto-updater (`macos.rs:857-864`).
- **Detection:**
  - `is_prelogin()` returns true when the active uid is 0 (`macos.rs:794-796`). The comment at `macos.rs:762` says "UID 0 owns /dev/console while the LoginWindow session is active".
  - `is_locked()` checks `ioreg` for `CGSSessionScreenIsLocked` (`macos.rs:798-828`).
  - `is_logon_screen` is prelogin or locked (`macos.rs:830-832`).
- **Capture and input:**
  - Capture uses `CGDisplayStream` (`libs/scrap/src/quartz/config.rs:37-40`, `quartz/ffi.rs:6`).
  - Input uses `CGEventPost` at the HID tap (`libs/enigo/src/macos/macos_impl.rs:78, 375`).
  - A commented-out block says "mouse/keyboard works in prelogin now with launchctl asuser" (`macos.rs:866`).
- **UNVERIFIED:** whether TCC (Screen Recording and Accessibility) actually lets capture and input work at the login window on current macOS. The docs at https://rustdesk.com/docs/en/client/mac/ cover only the Accessibility, Screen Recording, and Input Monitoring permissions. They say nothing about the login window or the lock screen.

## 3. Linux X11: display-manager login screen

- **Supported on X11.** The root `--service` (`src/platform/linux.rs:1124`) polls `loginctl` for the active seat0 session (`libs/base/src/platform/linux.rs:158-226`). It builds a `Desktop` with the session's uid, user, DISPLAY, and XAUTHORITY (`src/platform/linux.rs:2182-2192, 2543-2600`).
- **How it finds the greeter's X server:**
  - The XAUTHORITY path comes from the `Xorg -auth` argument of that user's Xorg process (`linux.rs:2401-2440`).
  - It falls back to `/run/user/<uid>/gdm/Xauthority`, `~/.Xauthority`, or `/var/lib/<user>/.Xauthority` (`linux.rs:2475-2498`). The last one matches LightDM's layout (my inference).
  - DISPLAY is found with `w <user>` or `/tmp/.X11-unix` (`linux.rs:2505-2530`). The comment "above not work for gdm user" is at `:2518`.
- **The helper runs as the greeter user.** If the session user is not root and not a Wayland greeter, the service starts `--server` as that user with DISPLAY and XAUTHORITY set (`linux.rs:1224-1243`; env setup at `932-975`). For an X11 greeter, that user is gdm, lightdm, or sddm. If the user is root, `--server` runs as root (`linux.rs:1177-1223`).
- **Input on X11:** enigo uses libxdo and XTest (`libs/enigo/src/linux/xdo.rs:12, 41`).
- **Docs:** "Remote access to the login screen still requires X11". The docs mention only GDM, with `WaylandEnable=false` (https://rustdesk.com/docs/en/client/linux/).

## 4. Linux Wayland

- **Capture uses xdg-desktop-portal and PipeWire through GStreamer `pipewiresrc`** (`libs/scrap/src/wayland/pipewire.rs:19-34, 291`). The code is derived from Weylus (`libs/scrap/src/wayland/README.md:3`).
- **Two portal modes, chosen by `is_server_running()`** (`pipewire.rs:1180-1189`):
  - **Installed (`--service` running):** uses the ScreenCast portal.
    - It asks for `types=1` (monitor) and `cursor_mode=2` (`pipewire.rs:939, 944`).
    - With ScreenCast portal version 4 or later, it sends `restore_token` (stored in the `wayland-restore-token` config key) and `persist_mode=2` (`pipewire.rs:760-761, 808-812, 921-928`).
    - **Input goes through uinput** virtual devices created by the root service: `start_uinput_service` (`src/platform/linux.rs:699-710`), `src/server/uinput.rs:4, 21-23, 725`, and `wayland_use_uinput()` = not X11 and `is_server()` (`src/server/input_service.rs:2569-2571`).
  - **Not installed:** uses the RemoteDesktop portal with `types=7` (keyboard, pointer, touchscreen) (`pipewire.rs:970-975`). The code comment says it has no restore_token or persist_mode: "TODO: support persist_mode for remote_desktop_portal" (`pipewire.rs:967`, `1187-1188`). Input goes through portal `notify_pointer_*` and `notify_keyboard_*` (`src/server/rdp_input.rs:245-480`; `input_service.rs:2575-2577`).
- **Lock screen: UNVERIFIED.** No Wayland-specific handling of the lock screen was found. `is_locked()` uses `loginctl LockedHint` (`src/platform/linux.rs:1446-1465`, `libs/base/src/platform/linux.rs:238-240`). The comment there says: "the 'Switch user' screen is usually Wayland login session, which we do not support" (`linux.rs:1442-1445`). Whether the portal stream keeps working while the screen is locked is not stated in the code.
- **Login greeter:**
  - **Default build:** refused. The connection is rejected with `"Wayland login screen is not supported"` (`src/client.rs:123`, `src/server/connection.rs:2057-2060`, `drm_can_serve_login_screen()` returns false at `connection.rs:520-524`). The docs agree: "Login screen using Wayland is not supported yet" (https://rustdesk.com/docs/en/client/linux/).
  - **Opt-in `drm` feature:** `Cargo.toml:33` (`drm = ["scrap/drm"]`), plus `drm-wake` (`Cargo.toml:34-39`) for synthetic wake. The feature works like this:
    - The root service reads KMS scanout through a dlopen'd `libdrmtap.so` (`libs/scrap/Cargo.toml:14-23`, `src/platform/linux.rs:1133-1153`) and streams it to a `--server` running as the greeter uid (`src/server/drm_capturer.rs:1-2`, `linux.rs:1191-1222`).
    - This path never talks to the compositor or the portal, "which is the entire reason it works at a login screen" (`linux.rs:2573-2580`).
    - CI ships it as a separate `rustdesk-unattended-wayland` deb (`.github/workflows/flutter-build.yml:2249-2256`).
    - Greeter detection matches only users `gdm` and `sddm` (`libs/base/src/platform/linux.rs:76-79`). The code notes that current GDM's `gdm-greeter` user does not match (`linux.rs:2581-2584`).
    - Greeter input presumably goes through the root uinput service (**UNVERIFIED**).
- **Compositors:** there is no explicit support list. The code has special cases for GNOME/Mutter (`libs/scrap/src/wayland/display.rs:151-155`; a GNOME 46 RDP-input bug at `src/server/rdp_input.rs:335-340`), KDE Plasma (`pipewire.rs:41-56, 1215`), COSMIC (`pipewire.rs:300-301`), Hyprland (`pipewire.rs:1351, 1832`), and niri (`rdp_input.rs:372`). There is no wlroots- or sway-specific code. Whether they work depends on the portal backend (**UNVERIFIED**).
- **Explicit messages in code:**
  - `"Wayland login screen is not supported"` (`client.rs:123`)
  - `wayland_experiment_tip`: "Wayland support is in experimental stage, please use X11 if you require unattended access." (`src/lang/en.rs:130`)
  - `wayland-requires-higher-linux-version` (`en.rs:124`)
  - `xdp-portal-unavailable` (`en.rs:125`)
  - `WAYLAND_UNSUPPORTED`, for a portal backend that is missing a capability (`src/server/wayland.rs:140, 197`)
  - "not supported on native Wayland" for the whiteboard (`connection.rs:5142`)
  - "Unsupported display server type" (`connection.rs:2063-2068`)

## 5. Capture and input crates: licenses and separability

| Crate | Path | License | Internal dependencies |
|---|---|---|---|
| `scrap` 0.5.0 (fork of quadrupleslap/scrap) | `libs/scrap` | `MIT` (`libs/scrap/Cargo.toml:8`). The crate has no LICENSE file. | `hbb_common`, `base` (`Cargo.toml:33-34`) |
| `enigo` 0.0.14 (fork of enigo-rs) | `libs/enigo` | `MIT` (`libs/enigo/Cargo.toml:15`, `libs/enigo/LICENSE`) | `hbb_common`, `base` (`Cargo.toml:27-28`), plus the rustdesk-org forks `rdev` and `tfc` |
| `base` | `libs/base` | no license field, so it falls under the repo-wide AGPL-3.0 (`LICENCE:1-2`) | `hbb_common` (`libs/base/Cargo.toml:18`) |
| `hbb_common` | git submodule `rustdesk/hbb_common` | no LICENSE file in the repo and no license field in its Cargo.toml (checked with `gh api repos/rustdesk/hbb_common/contents`). **UNVERIFIED** | n/a |
| App code: capture services, uinput, portal input, DRM consumer | `src/server/*`, `src/platform/*` | AGPL-3.0 (`LICENCE`) | n/a |

- **Wayland code in scrap:** it is "Derived from ... Weylus" (`libs/scrap/src/wayland/README.md:3`). Weylus is AGPL-3.0-or-later (from its LICENSE via the GitHub API). The MIT label on `scrap` may therefore not cover `scrap/src/wayland` (**UNVERIFIED** legal reading).
- **Separability:**
  - Neither crate depends on Flutter. The only "flutter" text is a comment at `libs/scrap/src/android/ffi.rs:472`.
  - Both use `hbb_common` (logging, `anyhow`, `config`, `tokio`, `libc`) and `base` (Linux platform helpers). Examples: `pipewire.rs:26-27`, `nix_impl.rs:125`, `macos_impl.rs:433`.
  - The scrap Wayland path also reads and writes RustDesk config (`config::LocalConfig` for the restore token, `pipewire.rs:923`) and checks whether the RustDesk service is running (`pipewire.rs:1189-1200`).
  - Both crates ship standalone examples: `libs/scrap/examples/*` and `libs/enigo/examples/*`.
  - Conclusion: they can be lifted out, but `hbb_common` and `base` (AGPL) must come along or be stubbed. The orchestration that handles login screens and desktop switching (service, helper, uinput, DRM) lives in AGPL app code, not in these crates.

## 6. CLI flags and headless use

These flags are parsed in `src/core_main.rs`:

- **Server and service roles:**
  - `--service`: runs as the OS service (`:381`).
  - `--server`: runs the capture and input server for one session (`:385-410`).
  - `--tray` (`:368`) and `--cm` (connection manager UI) (`:701`).
  - `--install-service` and `--uninstall-service` (`:373-380`).
  - `--portable-service` (`:318`), `--elevate` and `--run-as-system` (`:83-88`).
- **Install:** `--silent-install` (`:284`), `--install` and `--noinstall` (`:139-150`), `--uninstall` (`:238`), `--update` (`:243`), `--after-install` and `--before-uninstall` (`:274-279`), and the driver helpers `--install-idd`, `--uninstall-amyuni-idd`, `--install-remote-printer`, `--uninstall-remote-printer`, `--uninstall-cert` (`:306-346`).
- **Config.** Except `--get-id`, these need the app installed and admin or root rights (`:425-680`):
  - `--password <pw>` and `--set-unlock-pin <pin>`
  - `--get-id` and `--set-id <id>`
  - `--config <encrypted>` and `--import-config <file>`
  - `--option <key> [value]`
  - `--assign --token ...` and `--deploy --token ... [--id]`
  - Some or all of these can be blocked by `is_cli_setting_change_disabled` (`:884-889`).
- **Client.** These open the GUI:
  - `--connect <id>`, `--play`, `--file-transfer`, `--view-camera`, `--port-forward`, `--terminal`, `--terminal-admin`, `--rdp`
  - modifiers: `--password <pw>`, `--relay`, `--request-approval`
  - They are turned into a URI and the Flutter GUI starts (`:70-77, 130, 757-800`).
- **Misc:** `--version`, `--build-date` (`:153-156`), `--get-id`, `--check-hwcodec-config` (`:686`), `--terminal-helper` (`:690`), `--whiteboard` (`:705`).
- **No headless client:** I found no mode that runs a client session without the GUI.
  - `--server` and `--service` are the unattended host modes.
  - On Linux, `--server` also launches `--tray` (`:386-395`). On macOS it starts a tray (`:403-407`).
  - `plug_in_headless` (virtual display) is Windows-only, IDD/Amyuni (`src/virtual_display_manager.rs:31-40`).
  - No Linux headless (virtual X session) module was found at this commit (**UNVERIFIED** that it was removed or moved).
- **Docs:** the docs list `--password`, `--get-id`, `--set-id`, `--silent-install`, `--config`, `--import-config` (https://rustdesk.com/docs/en/client/).
