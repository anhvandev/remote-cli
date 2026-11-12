# Công cụ computer-use mã nguồn mở dạng CLI cho coding agent (2026-11)

Ngày tra cứu: 2026-11-12. Mọi nguồn được đọc trong ngày này.
Phạm vi: các CLI mà một coding agent (Claude Code, Codex, …) gọi từ shell, **mỗi lệnh làm một việc** (screenshot, click, type, key, list windows/focus app, đọc cây accessibility) và in ra kết quả máy đọc được. Câu hỏi chính: CLI nào chạy được trên Device qua `rcli exec`, và so với `rcli mcp` thì nó thêm được gì. Framework agent, model open-weight và sandbox được dời xuống mục "Ngoài phạm vi / tham khảo".

Nguồn chính:
- Repo này tại commit `5fb386e` (2026-11-12): `crates/cli/src/mcp.rs`, `crates/agent/src/{session,windows_session,windows_platform,macos}.rs`, `crates/agent/src/linux/{mod,shell}.rs`, `crates/protocol/proto/agent.proto`, `docs/usage.md`.
- README, LICENSE và docs trong repo GitHub, đọc qua `gh api repos/<r>/readme` và `gh api repos/<r>/license`. `pushed_at`, `archived` và `releases/latest` lấy từ GitHub API. Version lấy từ npm registry (`dist-tags.latest`, `time`) và PyPI JSON.
- Docs của chính tác giả: agent-browser.dev/desktop (agent-desktop), `libs/cua-driver/rust/Skills/cua-driver/{WINDOWS,LINUX}.md` và `scripts/docs-generators/headers/cua-driver/mcp-tools.md` trong `trycua/cua`, `docs/permissions.md` của Peekaboo, nirsoft.net/utils/nircmd.html.
- Orca: binary đã cài `/Applications/Orca.app` 1.4.256 (`orca computer --help`, `orca skills get computer-use`, `orca computer capabilities --json`).
- Model card trên Hugging Face (`/api/models/<id>` và `README.md`), chỉ cho mục tham khảo.

Không cài và không chạy CLI nào trên Device. Ngoài Orca, không chạy tool nào trên máy này. "Qua `rcli exec` được không" là suy luận từ code rcli cộng docs của tool, nên phần lớn ghi **UNVERIFIED**. Claim nào chưa xác nhận được thì ghi **UNVERIFIED**.

---

## 0. Điều kiện chạy trên Device: `rcli exec` chạy ở đâu

### 0a. Tool thật của `rcli mcp`
Brief nhắc `input`, `mark`, `clip`, nhưng `crates/cli/src/mcp.rs:66-104` không có tool nào tên như vậy. Tool thật là `devices`, `device`, `lock_acquire`/`lock_release`/`lock_status`, `observe`, `act`, `batch`, `exec`, `wake`, `power`. Ba tên trong brief ứng với:
- "input" là `act` (một Action) và `batch` (nhiều Action). Action gồm `move|click|double_click|right_click|drag|scroll|type|key|wait` (`mcp.rs:44-48`).
- "mark" là `marks: true` trên `observe`/`act`/`batch`, trả về bảng Mark. Mark chỉ có `number, text, role, display, x, y, width, height`, không có state hay value. Chi tiết ở [ai-device-level-e2e-testing.md §0b](ai-device-level-e2e-testing.md#0b-agent-nhìn-thấy-gì).
- "clip" là `observe: "clip"`, MP4 tối đa 5 s.

CLI tương ứng là `rcli shot`, `rcli act`, `rcli batch`, `rcli displays`, `rcli exec`, `rcli shell` (`docs/usage.md:129-207`).

### 0b. `exec` chạy trong Agent service, không chạy trong phiên GUI
- `SessionPlatform` gửi "screen calls … to the session helper on the console, the rest to `inner`" (`crates/agent/src/session.rs:83`). `shell()` và `pty()` gọi thẳng `self.inner` (`session.rs:194-199`). Vậy `exec` chạy trong tiến trình service root/SYSTEM, còn session helper chỉ lo capture, input và a11y.
- **Windows:** "The installed Agent is a SYSTEM service in session 0, which cannot see or type on the interactive desktops" (`windows_session.rs:2`). `shell()` chạy `powershell.exe` và có ghi chú `shortcut: runs as the account the Agent runs as, whatever account says` (`windows_platform.rs:347-350`). Hệ quả: **mọi CLI GUI gọi qua `rcli exec` trên Windows đều chạy trong session 0**. agent-desktop ghi rõ "Session 0 (services and SYSTEM tasks) … not supported" (https://agent-browser.dev/desktop/platforms/windows). Cua Driver ghi "UIA enumeration, screenshot via PrintWindow … silently return empty / timeout in Session 0" (`WINDOWS.md`, `trycua/cua` HEAD 2026-11-12).
- **macOS:** service là LaunchDaemon root. Lệnh của Terminal User chạy bằng `sudo -n -u <user> -i /bin/zsh -c …` (`macos.rs:278-298`), nên không có app cha nào mang TCC grant. Peekaboo cảnh báo: với "SSH, LaunchAgent, Codex, and other background launchd sessions … CoreGraphics can otherwise report success while returning only the desktop wallpaper or a redacted image" (`openclaw/Peekaboo` `docs/permissions.md`).
- **Linux:** service là systemd unit root. Lệnh chạy bằng `sudo -n -H -u <user> -- …` (`linux/shell.rs:22-38`) và không đặt `DISPLAY`, `XAUTHORITY`, `WAYLAND_DISPLAY` hay `DBUS_SESSION_BUS_ADDRESS`. agent-desktop yêu cầu "Run agent-desktop from a shell inside that session … an SSH shell without those variables … [is not supported]" (https://agent-browser.dev/desktop/platforms/linux). Agent phải tự export các biến này trong lệnh `exec`.
- **Ảnh qua `exec`:** `stdout` là `string` (`agent.proto:385`) và được decode bằng `String::from_utf8_lossy` (`windows_platform.rs:361`, `linux/shell.rs:74`, `macos.rs:568`). PNG in thẳng ra stdout sẽ hỏng, nên phải `base64` rồi decode ở phía agent. Giới hạn kích thước stdout của `exec`: **UNVERIFIED**.
- `rcli` không có lệnh copy file (`docs/usage.md` không có `cp`/`download`). Ảnh mà CLI lưu ra file trên Device chỉ lấy về được qua `exec … base64 <file>`.

---

## 1. macOS

### 1a. Peekaboo (`openclaw/Peekaboo`)
- MIT (LICENSE). v4.9.8 (2026-11-05), push 2026-11-08, không archived. Chỉ macOS 15+ (README).
- **Lệnh:** `peekaboo see --app Finder --json` trả "a structured UI map with opaque element IDs". `see --mode screen --path /tmp/x.png` chụp ảnh. Ngoài ra có `click "<label>" --app … --window-id …`, `type`, `press Return`, `scroll`, `drag`, `set-value`, `action`, `window list --json`, `app`, `menu`, `dock`, `dialog` (README, mục Command map).
- **Đầu ra:** JSON (`--json`) và ảnh PNG theo `--path`.
- **A11y:** có (AX). Hành động semantic, input chạy "background delivery" khi resolve được process (README).
- **Quyền:** Screen Recording, Accessibility, cùng "the additional permission used for synthetic input" (README; `docs/permissions.md`).
- **SSH/launchd:** có đường "Bridge". Quyền được cấp cho host Bridge (Peekaboo.app hoặc daemon), và docs khuyên dùng Bridge cho "SSH, LaunchAgent, Codex" (`docs/permissions.md`). Đây là CLI macOS duy nhất trong danh sách có tài liệu riêng cho trường hợp gọi từ phiên nền. Việc gọi qua `rcli exec` (sudo từ LaunchDaemon) có vào được Bridge socket của user hay không: **UNVERIFIED**.

### 1b. AXorcist / `axorc` (`openclaw/AXorcist`)
- MIT. v0.2.3 (2026-10-23), push 2026-11-05. macOS 14+ (README).
- **Lệnh:** `axorc tree --app Safari --depth 3`, `axorc find --app Safari --role AXButton --title Back`, `axorc raw --stdin` (JSON command có `performAction`/`AXPress`). Lọc được theo `AXValue`, `AXEnabled`, `AXFocused`, … (README).
- **A11y:** chỉ có a11y (đọc cây và AX action). Không chụp ảnh, không có input pixel.
- **Quyền:** Accessibility (`axorc permissions`). Chạy qua `rcli exec`: **UNVERIFIED** (TCC gắn với app cha, xem 0b).

### 1c. cliclick (`BlueM/cliclick`)
- License BSD-style, "Copyright (c) 2007-2018, Carsten Blüm" (LICENSE; GitHub ghi `NOASSERTION`). Release 5.1 (2022-08-14), push 2025-08-23. macOS 10.15+ (README).
- **Lệnh:** `cliclick c:123,456`, `dc:`, `rc:`, `m:x,y`, `dd:`/`du:` (drag), `t:text`, `kp:key`, `kd:`/`ku:`, `w:ms`. `-m verbose` in ra từng thao tác (README).
- **Đầu ra:** text. Không có a11y, không chụp ảnh. Chụp ảnh dùng `screencapture` có sẵn của macOS.
- **Quyền:** Accessibility cho terminal. "It is not possible to use cliclick before a user logs in, i.e.: to control the login window" (README). Qua `rcli exec`: **UNVERIFIED**, dễ thiếu TCC.

### 1d. Có sẵn trong macOS: `screencapture`, `osascript`
- `screencapture -x <file>` và `osascript` (System Events UI scripting) không cần cài thêm. Cần Screen Recording và Accessibility/Automation. Gọi từ LaunchDaemon có thể chỉ ra wallpaper (cảnh báo của Peekaboo ở 0b). Cách dùng qua `rcli exec`: **UNVERIFIED**.
- `steipete/macos-automator-mcp` (AppleScript/JXA) **đã archived** (GitHub API).

### 1e. Orca `orca computer` (`stablyai/orca`)
- MIT. v1.4.257 (2026-11-12). Là một subcommand của app desktop Orca (Electron), không phải binary riêng.
- **Lệnh:** `list-apps`, `list-windows`, `get-app-state --app <bundle> [--query] --json`, `click --element-index N | --x --y`, `set-value`, `type-text`, `press-key`, `hotkey`, `paste-text`, `scroll`, `drag`, `perform-secondary-action` (`orca computer --help`).
- **Đầu ra:** JSON. Cây nằm ở `result.snapshot.treeText` (có diff `treeDiff`), ảnh ở `result.screenshot.path` hoặc base64. Mỗi action có trạng thái xác minh `verified` / `unverified (…)` (`orca skills get computer-use`).
- **A11y:** có. Trên macOS input chạy background. Trên Linux và Windows thì "keyboard and pointer input still need the target window in front" (cùng nguồn). `capabilities` trên máy này báo provider `orca-computer-use-macos`.
- **Hạn chế cho rcli:** cần app Orca đang chạy trên Device. Không có tài liệu về việc chạy headless hay qua SSH: **UNVERIFIED**.

---

## 2. Linux

### 2a. xdotool (`jordansissel/xdotool`)
- BSD-3-Clause. v4.20260303.1 (2026-03-03), push 2026-09-20.
- **Lệnh:** `xdotool type "…"`, `key ctrl+c`, `mousemove x y click 1`, `search --name …`, `getactivewindow`, `windowactivate`. In window id dạng text (README, `xdotool.pod`).
- **Chỉ X11:** "If you are using Wayland … this software will not work correctly" (README). Không có a11y, không chụp ảnh. Chụp ảnh thì dùng `scrot` (MIT-style, 2.0.0, 2026-03-14) hoặc `import`.
- **Qua `rcli exec`:** cần `DISPLAY=:0 XAUTHORITY=<cookie của phiên>` trong lệnh. Bản thân rcli cũng tự tìm cookie này (`linux/session.rs:2-6, 119-128`). Chạy được hay không: **UNVERIFIED**.

### 2b. Wayland: ydotool, wtype, dotool, grim, kdotool
- **ydotool** (`ReimuNotMoe/ydotool`): **AGPL-3.0**, cùng license với repo này nên không xung đột. Release cuối v1.0.4 (2023-01-30), push 2025-12-22. Đi qua uinput nên chạy được trên X11, Wayland và console. `ydotoold` "requires access to `/dev/uinput`. **This usually requires root permissions.**" Không có window list, không có a11y (README). Qua `rcli exec --as root` thì quyền uinput là có, nhưng Agent rcli đã tự dùng uinput cho input rồi (`linux/uinput.rs`).
- **wtype** (`atx/wtype`): MIT, v0.5 (2026-10-20). "xdotool type for wayland", ví dụ `wtype -M ctrl c -m ctrl` (README). Chỉ gửi phím. Theo computer-use-linux, wtype không dùng được trên GNOME, KDE và COSMIC (README `agent-sh/computer-use-linux`).
- **dotool** (`git.sr.ht/~geb/dotool`): **GPL-3.0** (LICENSE). Ngày hoạt động gần nhất: **UNVERIFIED**.
- **grim** (`gitlab.freedesktop.org/emersion/grim`): MIT, tag v1.5.0 (2025-07-06), hoạt động 2026-11-12 (GitLab API). Chụp ảnh cho compositor wlroots. Hỗ trợ GNOME/KDE: **UNVERIFIED**.
- **kdotool** (`jinliu/kdotool`): Apache-2.0, v0.3.0 (2026-09-09). Clone của xdotool cho KDE Plasma 6, list và activate window qua KWin scripting. "Not all xdotool commands are supported" (README).

### 2c. computer-use-linux (`agent-sh/computer-use-linux`)
- MIT. v0.7.52 (2026-11-11). Có trên npm `@agent-sh/computer-use-linux` 0.7.52 và crates.io.
- **CLI chỉ có lệnh quan sát:** `doctor` (JSON readiness), `setup`, `apps`, `state [APP_NAME]`, `screenshot` ("JSON screenshot summary"), `windows`. Hành động (`click`, `type_text`, `press_key`, `perform_action`, `set_value`, …) **chỉ có qua MCP** (`computer-use-linux mcp`, hoặc `--listen` HTTP có bearer token) (README, mục CLI và mục MCP tools).
- **A11y:** AT-SPI (`get_app_state` cho cây thu gọn có `element_index`) cùng ảnh. Wayland dùng uinput hoặc portal RemoteDesktop. Window targeting biết từng compositor: GNOME extension, KWin, Hyprland, niri, Sway/i3, … (README, Support matrix).
- **Quyền:** bật AT-SPI (`gsettings toolkit-accessibility`). ydotoold chạy như service theo user (README).
- Dùng được với rcli chủ yếu ở chế độ `mcp --listen` cộng port forward. CLI `state`/`screenshot` qua `exec` cần biến môi trường của phiên: **UNVERIFIED**.

---

## 3. Windows

### 3a. AutoHotkey v2 (`AutoHotkey/AutoHotkey`)
- GPL-2.0. v2.0.31 (2026-10-25), push 2026-11-12.
- Không phải CLI một-lệnh-một-việc. Agent viết script `.ahk` rồi chạy (`Click`, `Send`, `WinGetList`, `ControlGetText`, …). Không có cây UIA sẵn. Cách in ra stdout cho agent: **UNVERIFIED** (chưa đọc docs).
- Qua `rcli exec`: session 0, xem 0b. Không dùng được nếu không có cách đẩy sang phiên user.

### 3b. Python-UIAutomation-for-Windows, `automation.py` (`yinkaisheng/…`)
- Apache-2.0. PyPI `uiautomation` 2.0.29 (2025-08-05), push 2026-06-02.
- **Lệnh dump cây UIA:** `automation.py -t 0 -n` ("print current active window's controls"), `automation.py -r -d 1 -t 0` (desktop và top-level windows). Kết quả in ra và lưu `@AutomationLog.txt` (README). Đầu ra là text, không phải JSON.
- Chỉ đọc. Hành động phải viết bằng Python API.

### 3c. NirCmd (nirsoft)
- **Freeware, không phải mã nguồn mở** ("This utility is released as freeware", https://www.nirsoft.net/utils/nircmd.html, v2.87).
- Lệnh: `nircmd.exe savescreenshot <file>`, `sendkeypress`, `setcursor`, …
- **Đáng chú ý cho rcli:** "if you try to use the savescreenshot command from a Windows service, you'll get a black image … You can solve this problem with runinteractivecmd". `runinteractive`/`runinteractivecmd` "only work from a Windows service ('SYSTEM' user)" (cùng trang). Đây chính là trường hợp của `rcli exec` trên Windows. Nó có thể khởi chạy một CLI khác (agent-desktop, cua-driver) trong phiên user, nhưng stdout của tiến trình đó không quay về `exec`, nên phải ghi ra file rồi đọc lại. Chưa thử: **UNVERIFIED**.

### 3d. Windows-MCP (`CursorTouch/Windows-MCP`)
- MIT. v0.8.9 (2026-11-12). Là MCP server (`uvx windows-mcp serve`, có `--transport sse|streamable-http`), không phải CLI một lệnh. `windows-mcp install` tạo Scheduled Task theo user (README). Tool và hành vi đã có ở [ai-device-level-e2e-testing.md §2b](ai-device-level-e2e-testing.md#2b-windows-mcp-cursortouch).
- Cách cài bằng Scheduled Task để vào được phiên user là mẫu nên học cho trường hợp session 0.

---

## 4. Đa nền tảng

### 4a. agent-desktop (`vercel-labs/agent-desktop`)
- Apache-2.0 (README, mục License; GitHub API). v1.2.0 (2026-11-05, npm `agent-desktop` 1.2.0), push 2026-11-11.
- **OS:** macOS 13+ (AX), Windows 10 1809+ (UIA), Ubuntu 24.04+ trên X11/Wayland (AT-SPI) (README, Platform support).
- **Lệnh:** 61 lệnh, gồm `snapshot --app Finder -i [--compact] [--skeleton]`, `find`, `get`, `is`, `screenshot`, `click @<snap>:eN`, `type`, `set-value`, `toggle`, `select`, `scroll`, `press`, `mouse-click`, `drag`, `list-apps`, `list-windows`, `focus-window`, `clipboard-get`, `wait`, `batch` (README, Commands).
- **Đầu ra:** "Each command prints one JSON envelope" `{version, ok, command, data|error}`. `error.disposition.retry` cho biết có nên thử lại không. Ref gắn với snapshot (`STALE_REF`, `AMBIGUOUS_TARGET`). Có thêm `agent-desktop mcp` (README). `screenshot` lưu file hay trả base64: **UNVERIFIED**.
- **A11y:** đây là cách quan sát chính. "without screenshots or pixel matching". Ref action "headless by default", tức không chiếm chuột hay focus (README).
- **Quyền và phiên:**
  - macOS: Accessibility cho app khởi chạy nó. "Under tmux or SSH there is no launching app, so add the terminal app that started tmux by hand" (https://agent-browser.dev/desktop/platforms/macos).
  - Windows: chỉ chạy trong phiên tương tác. Session 0, secure desktop và màn khoá đều "not supported" (…/platforms/windows).
  - Linux: chỉ chạy trong phiên GUI có session bus và AT-SPI bus. Không chạy được trong "SSH shell without those variables" (…/platforms/linux).

### 4b. Cua Driver, `cua-driver <tool> '<json>'` (`trycua/cua`, `libs/cua-driver`)
- Driver mang license MIT. Extension perception tùy chọn là **AGPL-3.0-only** (icon detector OmniParser) (README driver). Release `cua-driver-rs-v0.47.19` (2026-11-12). Chạy trên macOS, Windows, Linux X11/Wayland (README, mục e2e runners).
- **Lệnh:** "Every tool also runs from the shell as `cua-driver <tool> '<json-args>'` (or `cua-driver call <tool>` through the daemon)" (`mcp-tools.md`). Tool gồm `list_apps`, `list_windows`, `get_window_state` (cây cùng ảnh), `get_desktop_state` (ảnh toàn màn hình), `zoom`, `click`, `type_text`, `press_key`, `hotkey`, `scroll`, `drag`, `set_value`. Không có tool `screenshot` riêng (README driver). Trên PowerShell thì pipe JSON vào: `'{"pid":6004}' | cua-driver list_windows` (`WINDOWS.md`).
- **Đầu ra:** JSON có `effect` (`confirmed|unverifiable|suspected_noop|partial|refused`), `verified` và `path` (rung giao hàng: `ax`, `cgevent`, `x11_atspi`, `msaa`, …) (`mcp-tools.md`). Định dạng ảnh khi gọi qua CLI: **UNVERIFIED**.
- **A11y:** có, kèm `element_token`. Chi tiết ở [ai-device-level-e2e-testing.md §2e](ai-device-level-e2e-testing.md#2e-cua-driver-trycua).
- **Phiên. Đây là tool có tài liệu rõ nhất cho trường hợp gọi từ ngoài phiên:**
  - **Linux:** "cua-driver now auto-discovers the session bus at startup (mirroring the `XAUTHORITY` recovery): if `DBUS_SESSION_BUS_ADDRESS` is unset it adopts `/run/user/<uid>/bus` …". Trường hợp được nêu tên gồm "`runuser`/`su` into the desktop user, a systemd system unit". Vẫn cần AT-SPI bus và `toolkit-accessibility` bật (`LINUX.md`). Đây đúng là trường hợp `rcli exec`.
  - **Windows:** runtime phải ở "Session 1+, NOT Session 0". "If you started the daemon via SSH-into-Windows, that session is usually Session 0: kick the autostart task instead: `schtasks /Run /TN cua-driver-serve`" (`WINDOWS.md`). Sau đó `cua-driver call` đi qua daemon. Gọi `call` từ SYSTEM/session 0 có tới được daemon của user hay không: **UNVERIFIED**.
  - **macOS:** quyền thuộc `CuaDriver.app`, và CLI proxy qua daemon của app đó. "Directly spawning a raw `cua-driver serve` outside `CuaDriver.app` … is unsupported" (README driver).

### 4c. usecomputer (`remorses/usecomputer`)
- MIT. v0.1.11 (2026-04-07, npm), push 2026-09-08. Backend native viết bằng Zig. Chạy macOS, Linux (X11 hoặc XWayland), Windows (README).
- **Lệnh:** `screenshot ./shot.png --json` trả `{path, coordMap, desktopIndex, hint}`, ảnh có cạnh dài ≤ 1568. Ngoài ra có `click -x -y --coord-map …`, `mouse move`, `mouse position --json`, `type "…"` (`--stdin`), `press "cmd+s"`, `drag`, `window list --json`, `screenshot --window <id>`, `debug-point` (README).
- **Chỉ pixel**, không có a11y.
- **Quyền:** macOS cần Accessibility. Linux cần `DISPLAY`. Windows cần "an interactive desktop session. Automation input is blocked on a locked desktop" (README). Qua `rcli exec` thì gặp đúng các giới hạn ở 0b.
- So với rcli: trùng vai trò với `rcli shot`/`rcli act` (cũng scale ảnh và tự đổi tọa độ), nhưng không chạy được ở màn khoá. Thêm nó vào không có lợi.

### 4d. Terminator CLI (`mediar-ai/terminator`)
- MIT. `@mediar-ai/cli` 0.24.32 (2026-04-05). Repo push lần cuối 2026-06-02. "Terminator currently supports **Windows only**" (README). Đã có ở [ai-device-level-e2e-testing.md §2d](ai-device-level-e2e-testing.md#2d-terminator-mediar-ai).

### 4e. Midscene / Stagehand CLI
- `@midscene/cli` 1.17.8 (2026-11-11, MIT) chạy file kịch bản YAML, không phải lệnh một-việc. Stagehand chỉ dành cho browser. Cả hai nằm ngoài phạm vi. Xem [ai-visible-e2e-testing.md §5, §7](ai-visible-e2e-testing.md#5-stagehand-browserbase) và [ai-device-level-e2e-testing.md §5](ai-device-level-e2e-testing.md#5-mobile).

---

## 5. Mobile (Device làm host, điện thoại cắm vào)
rcli không có Agent cho Android hay iOS. Tuy vậy, một Device có cắm điện thoại có thể chạy các CLI này qua `rcli exec`.
- **adb** (AOSP, Apache-2.0): `adb exec-out screencap -p`, `adb shell uiautomator dump`, `adb shell input tap x y | text … | keyevent …`. Ảnh nhị phân phải đi qua `base64` (0b). Chưa đọc lại docs: **UNVERIFIED** cho cú pháp cụ thể.
- **AXe** (`cameroncooke/AXe`): MIT, v2.0.4 (2026-11-12). iOS Simulator và cả máy thật đã pair. Lệnh `axe describe-ui --udid … ` ("accessibility tree as JSON"), `axe tap --label …`, `axe type …`, `axe screenshot --output screen.png` (in ra đường dẫn file). Lỗi có code (`elementNotFound`, `ambiguousElement`, …) (README). Cần macOS và Xcode.
- **idb** (`meta/idb`, trước là `facebook/idb`): MIT, v1.7.8 (2026-11-01). CLI cho iOS Simulator và iOS device (README).
- **mobilecli** (`mobile-next/mobilecli`): **FSL-1.1-ALv2, không phải license OSI** (LICENSE: "Functional Source License, Version 1.1, ALv2 Future License"). 1.0.23 (2026-11-11). Chạy iOS (thật và simulator), Android (thật và emulator): screenshot, tap, nút cứng, app, log (README).

---

## 6. Bảng so sánh

| CLI | Lệnh chính | Đầu ra | Cây a11y | OS | License | Quyền / phiên | Hoạt động gần nhất |
|---|---|---|---|---|---|---|---|
| **agent-desktop** | `snapshot -i`, `click @ref`, `type`, `set-value`, `press`, `screenshot`, `list-windows`, `focus-window` | JSON envelope (+ MCP) | có (AX/UIA/AT-SPI), ref theo snapshot | macOS 13+, Win10 1809+, Ubuntu 24.04+ X11/Wayland | Apache-2.0 | AX cho app cha; Windows không chạy ở session 0; Linux cần env phiên | v1.2.0, 2026-11-05 |
| **cua-driver** | `cua-driver <tool> '<json>'`: `get_window_state`, `get_desktop_state`, `click`, `type_text`, `hotkey`, … | JSON có `effect`/`verified` | có, `element_token` | macOS, Windows, Linux X11/Wayland | MIT (extension perception AGPL) | macOS qua `CuaDriver.app`; Windows cần daemon session 1+ (`schtasks /Run`); Linux tự tìm session bus | v0.47.19, 2026-11-12 |
| **Peekaboo** | `see --json`, `click`, `type`, `press`, `window list`, `menu`, `dialog` | JSON + PNG `--path` | có (AX) | macOS 15+ | MIT | Screen Recording, AX, synthetic input; Bridge cho SSH/launchd | 4.9.8, 2026-11-05 |
| Orca `orca computer` | `get-app-state --json`, `click --element-index`, `set-value`, `type-text`, `hotkey` | JSON (`treeText`, `screenshot.path`) | có | macOS (bg), Linux/Windows (fg) | MIT | cần app Orca chạy | v1.4.257, 2026-11-12 |
| axorc | `tree`, `find`, `raw --stdin` | text/JSON | có (chỉ a11y) | macOS 14+ | MIT | AX | v0.2.3, 2026-10-23 |
| cliclick | `c:x,y`, `t:text`, `kp:key`, `dd:/du:` | text | không | macOS 10.15+ | BSD-style | AX; không chạy ở login window | 5.1, 2022-08 (push 2025-08) |
| computer-use-linux | CLI: `doctor`, `apps`, `state`, `screenshot`, `windows`; action chỉ qua MCP | JSON | có (AT-SPI) | Linux X11/Wayland (GNOME, KDE, Hyprland, niri, Sway, …) | MIT | AT-SPI bật; ydotoold user service | v0.7.52, 2026-11-11 |
| xdotool | `type`, `key`, `mousemove … click`, `search`, `getactivewindow` | text | không | Linux X11 | BSD-3-Clause | `DISPLAY` + `XAUTHORITY` | v4.20260303.1, 2026-03-03 |
| ydotool | `type`, `key`, `mousemove`, `click` | không | không | Linux (uinput) | AGPL-3.0 | `/dev/uinput`, thường cần root | v1.0.4, 2023-01 (push 2025-12) |
| wtype | `wtype text`, `-M ctrl c -m ctrl` | không | không | Wayland (không GNOME/KDE) | MIT | phiên Wayland | v0.5, 2026-10-20 |
| kdotool | các lệnh window của xdotool | text | không | KDE Plasma 6 | Apache-2.0 | session D-Bus KWin | v0.3.0, 2026-09-09 |
| grim | `grim out.png` | PNG | không | Wayland wlroots | MIT | phiên Wayland | v1.5.0, 2025-07 |
| usecomputer | `screenshot --json`, `click --coord-map`, `type`, `press`, `window list` | JSON + PNG | không | macOS, Linux X11, Windows | MIT | AX (macOS), phiên tương tác | v0.1.11, 2026-04-07 |
| `automation.py` (uiautomation) | `-t 0 -n`, `-r -d 1 -t 0` | text + `@AutomationLog.txt` | có (UIA, chỉ đọc) | Windows | Apache-2.0 | phiên tương tác | 2.0.29, 2025-08 (push 2026-06) |
| AutoHotkey v2 | script `.ahk` | tùy script | không sẵn | Windows | GPL-2.0 | phiên tương tác | v2.0.31, 2026-10-25 |
| NirCmd | `savescreenshot`, `sendkeypress`, `runinteractivecmd` | file | không | Windows | **freeware, đóng** | `runinteractive*` chỉ từ service SYSTEM | v2.87 |
| AXe | `describe-ui`, `tap --label`, `type`, `screenshot --output` | JSON + path | có (iOS) | macOS host → iOS sim/device | MIT | Xcode | v2.0.4, 2026-11-12 |
| mobilecli | screenshot, tap, button, app, logs | text/JSON | **UNVERIFIED** | host → iOS/Android | **FSL-1.1-ALv2** | adb / Xcode | 1.0.23, 2026-11-11 |

---

## 7. Ngoài phạm vi / tham khảo

Phần này chỉ để tra cứu, không đọc sâu. Mỗi dòng có link nguồn chính.

### 7a. Framework và harness agent
- **Agent S** (`simular-ai/Agent-S`, Apache-2.0, `gui-agents` 0.3.2 2025-12-16, S4 2026-10-20, push 2026-10-30): Win/macOS/Linux, pixel cộng grounding model. Đã có ở [ai-device-level §1e](ai-device-level-e2e-testing.md#1e-agent-s-simular).
- **UFO²/UFO³** (`microsoft/UFO`, MIT, v3.0.10 2026-09-22): Windows, UIA cùng vision. Xem [ai-device-level §2a](ai-device-level-e2e-testing.md).
- **Midscene** (`web-infra-dev/midscene`, MIT, v1.17.8): chỉ nhìn ảnh, có `AbstractInterface` để cắm backend tùy ý. Xem [§5](ai-device-level-e2e-testing.md#5-mobile) và [mục "Phù hợp với repo này" §2](ai-device-level-e2e-testing.md).
- **OpenCUA agent** chạy trong OSWorld (`xlang-ai/OpenCUA`, MIT; agent ở `xlang-ai/OSWorld/mm_agents/opencua_agent.py`, Apache-2.0).
- **OpenAdapt** (`OpenAdaptAI/OpenAdapt`, MIT, `openadapt-flow-v2.7.1` 2026-11-10).
- **e2b open-computer-use** (`e2b-dev/open-computer-use`, Apache-2.0, push 2026-10-23).
- **MobileAgent / Mobile-Agent-v3.5** (`X-PLUG/MobileAgent`, MIT, push 2026-11-03).
- **Goose** (`aaif-goose/goose`, Apache-2.0, v1.59.0 2026-11-10): agent dạng MCP client tổng quát.
- **hai-agents** (`hcompai/hai-agents-python`, SDK MIT, v1.5.0 2026-11-10): SDK cho Agents API của H Company. Chạy local cần `hai-agent-runtime` (license **UNVERIFIED**). Inference mặc định chạy hosted (README).
- **Fara CLI / MagenticLite** (`microsoft/fara`, MIT) và **Qwen-CUA demo** (`xlang-ai/Qwen-CUA`, Apache-2.0): chỉ browser.
- **self-operating-computer** (`OthersideAI/self-operating-computer`, MIT): release cuối v1.5.8 (2025-02-28), push 2026-10-19.

### 7b. Model open-weight (số do tác giả tự công bố, lấy theo model card)

| Model | Kích thước | License | Số đã đọc | Nguồn |
|---|---|---|---|---|
| UI-TARS-1.5-7B | 8.3B | Apache-2.0 (repo **archived**) | OSWorld 27.5, ScreenSpot-Pro 49.6 | HF card, 2025-04 |
| OpenCUA-7B / 32B / 72B | 8.3B / 33B / 73B | MIT | OSWorld-Verified (100 step) 26.6 / 34.8 / 45.0; ScreenSpot-Pro 50.0 / 55.3 / 60.8; 72B mẫu dùng 8 GPU TP | HF card (72B sửa 2026-11-09) |
| Qwen-CUA | 397B, 17B active | Apache-2.0 | OSWorld-Verified 86.2; OSWorld 2.0 binary 18.5; ~807 GB BF16, tham chiếu 8×H200 | HF card, 2026-08 |
| EvoCUA-8B / 32B | 8.8B / 33B | Apache-2.0 | OSWorld (50 step) 46.1 / 56.7; WAA 56.48 (32B) | HF card, 2026-01 / 2026-03-31 |
| GUI-Owl-1.5-8B-Instruct / 32B-Instruct | 8.8B / 33B | MIT | OSWorld-Verified 52.3 / 56.5; WindowsAA 31.7 / 44.8; AndroidWorld 69.0 / 69.4; đã kiểm trên A100 96 GB | HF card, 2026-02 |
| Holo4-35B-A3B | 35B, 3B active | Apache-2.0 | OSWorld 80.8%; OSWorld 2.0 30.9% (score) / 12.3% (success); AndroidWorld 77.6% (chạy trong harness của H) | HF card + bảng ảnh, 2026-09-28 |
| Holo4-27B | 27B | **CC-BY-NC-4.0 (cấm thương mại)** | OSWorld 85.2%; OSWorld 2.0 61.7% / 41.5% | như trên |
| Holotron4-30B-A3B | 33B | **NVIDIA Open Model Agreement (không OSI)** | OSWorld 76.3; OSWorld 2.0 7.9 | HF card, 2026-09-28 |
| Holo2-4B / 8B (30B-A3B) | 4B / 8.8B | Apache-2.0 (**30B-A3B research-only**) | Holo2-8B OSWorld 39.9%, ScreenSpot-Pro 58.9% | HF card |
| Holo-3.1 (0.8B–35B-A3B) | — | Apache-2.0 | bảng số là ảnh, chưa đọc: **UNVERIFIED** | HF card, 2026-06 |
| UI-Venus-1.5-2B / 8B / 30B-A3B | — / 8.8B / 31B | Apache-2.0 | ScreenSpot-Pro 57.7 / 68.4 / 69.6; AndroidWorld 77.6 | HF card, 2026-02 |
| UI-Venus-2-9B | 9B | **chưa có**: "license is pending final confirmation" | OSWorld-Verified 70.8; ScreenSpot-Pro 73.0; AndroidWorld 80.2 | HF card, 2026-09 |
| MAI-UI-2B / 8B | 2B / 8.8B | Apache-2.0 (HF) | ScreenSpot-Pro 73.5, AndroidWorld 76.7 (card không ghi size nào: **UNVERIFIED**) | HF card, 2026-01 |
| GTA1-7B / 32B (Salesforce) | 7B / 33B | MIT | ScreenSpot-Pro 55.5 / 63.6; GTA1-32B + GPT-5 OSWorld-Verified 63.4, WAA 50.6 | HF card, 2025-10 |
| Fara1.5-4B / 9B / 27B | 4B / 9.4B / 27B | MIT | chỉ web (WebVoyager 80.8 / 86.6 / 89.3) | HF card, 2026-05 |
| ShowUI-2B, ShowUI-π | 2B, 450M | MIT / license không ghi | — | HF, 2025-03 / 2026-04 |
| UGround-V1-7B / 72B | 8.3B / 73B | Apache-2.0 / **tongyi-qianwen (72B)** | — | HF card |
| OS-Atlas-Pro-7B, ScaleCUA, Phi-Ground-Any-4B | — | Apache-2.0 / Apache-2.0 / MIT | — | HF |
| OmniParser v2 | — | `icon_detect` **AGPL-3.0**; `icon_detect_v3` + `icon_caption` MIT | ScreenSpot-Pro 39.6 | HF card, 2026-09-23 |

Về license so với AGPL-3.0 của repo này: AGPL, Apache, MIT, BSD và GPL-3.0 đều dùng được khi tool chạy như chương trình riêng trên Device. Những thứ cần chú ý:
- Cấm thương mại: Holo4-27B, Holo2-30B-A3B, Holo1.5-72B.
- License riêng: NVIDIA (Holotron4), tongyi-qianwen (UGround-72B).
- Chưa có license: UI-Venus-2.
- Source-available, không phải OSI: FSL (mobilecli, Cua Spaces).
- Đóng: NirCmd (freeware).

### 7c. Sandbox, parser, MCP server
- OmniParser, cua sandbox/Lume/Bench, E2B Desktop, OSWorld `desktop-env`, WindowsAgentArena: xem [ai-device-level §2f, §3](ai-device-level-e2e-testing.md#3-sandbox-và-harness-chạy-agent-trên-vm-đầy-đủ).
- `huggingface/screenenv` (MIT, push 2026-09-24): sandbox Docker Linux cùng MCP.
- MCP desktop: Windows-MCP, MacOS-MCP, Peekaboo, Cua Driver (xem [§2b–2e](ai-device-level-e2e-testing.md#2-tự-động-hoá-desktop-cho-agent)). Ngoài ra có `domdomegg/computer-use-mcp` (MIT, v1.8.0 2026-04-13).

---

## Đã ngừng
- `bytedance/UI-TARS`, `bytedance/UI-TARS-desktop`: archived (GitHub API).
- `bytebot-ai/bytebot`: archived, push cuối 2025-09-12.
- `steipete/macos-automator-mcp`: archived.
- `kortix-ai/agent-computer-use` (`agent-cu`): README ghi "**This project is no longer maintained.** We recommend Cua".
- `baryhuang/mcp-remote-macos-use` (điều khiển macOS qua VNC): push cuối 2025-06-10.
- `BAAI-Agents/Cradle` (2024-11) và `OS-Copilot/OS-Copilot` (2024-09).
- Chậm lại nhưng chưa ngừng: `mediar-ai/terminator` (push 2026-06-02), `ydotool` (release cuối 2023-01).

---

## Chạy trên rcli

Các dữ kiện quyết định:
- `rcli mcp` đã có screenshot, input, Mark và clip. Nó chạy được **cả ở màn khoá, màn đăng nhập và UAC**, mà không cần cài gì thêm trên Device.
- Thứ CLI ngoài thêm được:
  - Cây a11y đầy đủ có state và value. Mark của rcli phẳng, không có state, và chỉ lấy cửa sổ foreground (Windows) hoặc app đang focus (macOS).
  - Hành động semantic (AXPress, UIA Invoke, `set-value`) có xác minh.
  - Danh sách cửa sổ và focus app.
  - Input chạy nền.
- Chặn lớn nhất là chỗ `exec` chạy (0b): trên Windows là session 0, trên macOS là LaunchDaemon không có TCC, trên Linux là sudo thiếu biến môi trường của phiên. Ảnh phải đi qua `base64`.

### 1. Cua Driver: ứng viên đầu tiên cho Linux, rồi Windows
- **Lý do:** MIT, cùng một CLI JSON cho cả ba OS. Tài liệu nêu đúng trường hợp gọi từ "systemd system unit" hoặc "`runuser`/`su`" trên Linux, và trường hợp SSH vào session 0 trên Windows. Mỗi action trả `effect`/`verified`.
- **Bước nhỏ nhất (Linux, X11 hoặc GNOME Wayland):**
  - Cài bằng `curl -fsSL https://cua.ai/driver/install.sh | bash` cho Terminal User.
  - Chạy `rcli exec <dev> -- cua-driver list_apps '{}'`, sau đó `cua-driver get_window_state '{"pid":…,"query":"…"}'`.
  - Đọc JSON trả về. Nếu cây rỗng thì kiểm `toolkit-accessibility`.
- **Bước nhỏ nhất (Windows):** cài theo user, bật autostart task. Từ `rcli exec` chạy `schtasks /Run /TN cua-driver-serve`, rồi `cua-driver call list_windows`.
- **Chặn đã biết:**
  - Windows: `exec` chạy dưới SYSTEM trong session 0, còn daemon chạy theo user. Việc `call` có kết nối được daemon hay không: **UNVERIFIED**.
  - macOS: cần `CuaDriver.app` có TCC, và proxy từ `sudo -u` trong LaunchDaemon: **UNVERIFIED**.
  - Cú pháp `cua-driver <tool> '<json>'` lấy từ docs, chưa chạy thử.
  - Không dùng được ở màn khoá. Không cài extension perception nếu không chấp nhận AGPL. Repo này cũng AGPL nên không xung đột, nhưng phải kiểm lại khi phân phối.

### 2. agent-desktop: CLI JSON đầy đủ nhất, hợp nhất cho Linux
- **Lý do:** Apache-2.0, 61 lệnh, JSON envelope có `retry` disposition, ref gắn snapshot, hành động semantic chạy headless, có skill sẵn cho Claude Code và Codex.
- **Bước nhỏ nhất (Linux):**
  - `npm i -g agent-desktop` trên Device.
  - Chạy `rcli exec <dev> -- env DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus agent-desktop status`, rồi `agent-desktop snapshot --app <App> -i --compact`.
  - Với X11 có thể cần thêm `XAUTHORITY`. Đường dẫn cookie phụ thuộc display manager (rcli tự tìm trong `linux/session.rs:119-128`).
- **Chặn đã biết:**
  - Windows: session 0 "not supported", nên phải đẩy sang phiên user (Scheduled Task, hoặc `nircmd runinteractive` vốn là freeware) rồi đọc kết quả từ file.
  - macOS: TCC gắn với "the app that launched agent-desktop", mà dưới `rcli exec` thì không có app nào như vậy: **UNVERIFIED**.
  - Định dạng ảnh của `screenshot`: **UNVERIFIED**.

### 3. Peekaboo: lựa chọn cho macOS
- **Lý do:** MIT, JSON cùng PNG, AX đầy đủ, menu và dialog. Có **Bridge** được thiết kế cho phiên SSH và launchd, đúng hoàn cảnh của `rcli exec` trên macOS.
- **Bước nhỏ nhất:**
  - Cài Peekaboo.app cùng CLI (`brew install openclaw/tap/peekaboo`), cấp Screen Recording và Accessibility cho Peekaboo.app trong phiên Aqua.
  - Chạy `rcli exec <mac> -- peekaboo permissions status --all-sources`. Kỳ vọng dòng `Source: Peekaboo Bridge`.
  - Sau đó `peekaboo see --app Finder --json`.
- **Chặn đã biết:**
  - Bridge socket có tìm được khi gọi qua `sudo -n -u <user> -i` từ LaunchDaemon hay không: **UNVERIFIED**.
  - Chỉ macOS 15+. Không chạy ở màn đăng nhập (rcli cũng chỉ xem được màn đăng nhập macOS, theo ADR 0001).

### So với `rcli mcp`
- Ở màn khoá, màn đăng nhập, UAC, khi boot hoặc wake: chỉ `rcli mcp` (`observe`/`act`/`batch` + `power`/`wake`) làm được. Không CLI nào ở trên chạy được ở đó.
- Trong app khi đã đăng nhập: CLI cho cây có state, hành động semantic và xác minh hiệu ứng, những thứ rcli chưa có ([ai-device-level §0f](ai-device-level-e2e-testing.md#0f-khoảng-trống-khi-dùng-làm-harness-e2e)). Cái giá là phải cài phần mềm trên từng Device và tự xử lý chỗ `exec` chạy ngoài phiên.
- CLI chỉ có pixel (usecomputer, cliclick, xdotool, ydotool, NirCmd) trùng vai trò với `rcli act`/`rcli shot`. Chúng không thêm được gì, và còn kém hơn ở màn khoá.
- Hướng sửa phía rcli nếu muốn mọi CLI GUI chạy được qua `exec`: cho `exec` một tuỳ chọn chạy qua session helper (phiên console có đủ `DISPLAY`/D-Bus, Aqua, hoặc session 1+). Đây là thay đổi code, nằm ngoài phạm vi research này.
