# Kiểm chứng bằng source code: 5 công cụ computer-use mã nguồn mở so với rcli (2026-11)

Ngày làm: 2026-11-13.
Mục đích: kiểm lại bằng **source code** (không dùng README) những claim về Cua Driver, agent-desktop, Peekaboo/AXorcist, Windows-MCP và UFO² trong hai file research trước ([ai-device-level-e2e-testing.md](ai-device-level-e2e-testing.md) §0, §2 và [open-source-computer-use.md](open-source-computer-use.md)). Sau đó so từng năng lực với code của rcli, theo kiểu code đối chiếu code, và rút ra phần nên nâng cấp.

## Nguồn và cách làm

| Repo | Commit | Ngày commit | License (theo code) |
|---|---|---|---|
| `trycua/cua` (`libs/cua-driver`) | `bc98739ff6589fdfecdd0004098d1f4d47b52a60` | 2026-11-13 | MIT (`libs/cua-driver/rust/Cargo.toml:25`). Weights detector của perception là AGPL-3.0 |
| `vercel-labs/agent-desktop` | `947b051fb7a03a797ad12b9bb942d4ffc6f91735` | 2026-11-12 | Apache-2.0 (`LICENSE`, `Cargo.toml:9`) |
| `openclaw/Peekaboo` | `69764083b16397845dc68b5c0ddb4f674290ae70` | 2026-11-08 | MIT (`LICENSE:1`) |
| `openclaw/AXorcist` | `83dcaaf8a01ffb64e8266a7041a6829b99d581b8` | 2026-11-06 | MIT (`LICENSE:1`) |
| `CursorTouch/Windows-MCP` | `106b50c4e50c6886c4b54a5707985e7b333f120d` | 2026-11-12 | MIT (`LICENSE.md:1`). Phần `uiautomation` vendored ghi Apache-2.0 (`src/windows_mcp/uia/core.py:10`) |
| `microsoft/UFO` | `eae3c0a8d168c47cb10df0652e293e092eda1078` | 2026-10-14 | MIT (`LICENSE`) |
| rcli (repo này) | `65609dd` | 2026-11-12 | AGPL-3.0 (ADR 0002) |

- Mỗi repo được shallow-clone vào một thư mục riêng dưới `/tmp/rcli-src-verify/`. Không URL nào đổi chỗ.
- **Không build, không cài, không chạy gì**, cả trong các repo ngoài lẫn trong rcli. Chỉ đọc file bằng `grep`, `sed`, `cat`, `find` và `git log -1`.
- Submodule `Peekaboo/AXorcist` trong bản shallow clone là thư mục rỗng. Code AX vì thế được đọc từ clone `openclaw/AXorcist` riêng. Peekaboo ghim AXorcist ở `826eed6` (gitlink) và `0.2.2` (`Package.swift:39`), đều khác `83dcaaf`. Số dòng của AXorcist là của `83dcaaf`.
- Năm repo ngoài được đọc song song bởi các agent con chỉ được phép đọc. Các claim chính được kiểm lại tay ở cuối, gồm:
  - `effect`/`verified` và `recording.rs` của Cua, `_interactive_desktop` của Windows-MCP, `FindAllBuildCache`+500 và `ui_trees/` của UFO;
  - `DesktopSessionLockState` và `SLEventPostToPid` của Peekaboo, envelope `2.6` của agent-desktop;
  - socket daemon và tên task autostart của Cua.

  Số dòng còn lại lấy từ báo cáo của các agent con, chưa mở lại từng dòng.
- Code rcli: `crates/agent/src/{platform,marks,observer,clip,coords,macos,macos_session,windows_platform,windows_session,session,executor,lib}.rs`, `crates/agent/src/linux/{atspi,mod,session,shell,x11,screencast,wayland,uinput}.rs`, `crates/protocol/proto/agent.proto`, `crates/cli/src/{mcp,lib,agent,macos_install}.rs`, `crates/server/src/api/actions.rs`, `docs/adr/0004…`, `docs/usage.md`. Có đọc thêm `.scratch/easy-setup/issues/{21,22}…md`. Thư mục này đang có thay đổi chưa commit của người khác; chỉ đọc, không sửa.
- Ký hiệu kết luận: **CONFIRMED** (code đúng như claim cũ), **REFUTED** (code làm khác, có ghi code làm gì), **NOT FOUND** (không có trong code), **UNVERIFIED** (code không đủ để kết luận, có ghi lý do).

---

## 1. Cua Driver (`trycua/cua`, `libs/cua-driver`)

Tiền tố đường dẫn trong mục này: `libs/cua-driver/rust/crates/`.

### 1.1 Cây accessibility
- **macOS (AX):**
  - Gốc là `AXUIElementCreateApplication(pid)` (`platform-macos/src/ax/tree.rs:288`). Con cấp cao là `AXChildren ∪ AXWindows`, vì app ở nền thì AXChildren rỗng (`tree.rs:195-206, 406-425`).
  - Đọc theo lô bằng `AXUIElementCopyMultipleAttributeValues` (`ax/batch.rs:25`), gồm 16 attribute: AXRole, AXTitle, AXValue, AXPlaceholderValue, AXSubrole, AXDescription, AXIdentifier, AXHelp, AXEnabled, AXSelected, AXPosition, AXSize, AXValueDescription, AXMinValue, AXMaxValue, AXChildren (`batch.rs:81-96`).
  - Danh sách action lấy từ `AXUIElementCopyActionNames` (`tree.rs:599`).
  - Giới hạn: depth 25, 2000 node, `AXUIElementSetMessagingTimeout` 2.0 s (`tree.rs:32,42,48`).
  - Với Electron/Chromium thì bật `AXManualAccessibility` (`ax/bindings.rs:843-854`).
  - Lọc theo cửa sổ bằng hàm private `_AXUIElementGetWindow` (`ax/bindings.rs:109`).
- **Windows (UIA):**
  - `CoCreateInstance(CUIAutomation)` trong MTA (`platform-windows/src/uia/mod.rs:300`).
  - Không dùng `FindAll`. Code gọi `ElementFromHandle(hwnd)` rồi `BuildUpdatedCache` với `TreeScope_Subtree` và `ControlViewCondition`, lấy cả cây con trong một lần gọi, thử lại 3 lần cách 40 ms (`mod.rs:353-357, 397-433`). Sau đó chỉ đi `GetCachedChildren`. Comment ghi lý do không dùng `ElementFromHandleBuildCache`: nó treo trên cửa sổ LibreOffice "SAL" (`mod.rs:392-396`).
  - CacheRequest gồm (`mod.rs:705-737`):
    - property: ControlType, Name, Value.Value, AutomationId, HelpText, IsEnabled, IsOffscreen, BoundingRectangle, Toggle.ToggleState, SelectionItem.IsSelected;
    - pattern: Invoke, Toggle, SelectionItem, ExpandCollapse, Value, RangeValue, Text, Scroll.
  - Giới hạn: depth 25, 5000 node (`mod.rs:44,46`). `BuildUpdatedCache` không ngắt được, nên task bị bỏ sau `timeout_ms + 500 ms` và trả cây rỗng có cờ `truncated` (`tools/get_window_state.rs:124, 400-428`).
  - Dự phòng:
    - cửa sổ `SAL*` dùng MSAA (`AccessibleObjectFromWindow`/`accLocation`, `uia/msaa.rs`);
    - CoreWindow/UWP có cây rỗng thì `FindAll(TreeScope_Children)` lọc theo ProcessId (`mod.rs:568`);
    - Chromium thì đi từng tầng, dừng ở `Document` (`mod.rs:740-790`).
- **Linux (AT-SPI):**
  - Crate `atspi` trên zbus (`platform-linux/src/atspi/native.rs:209`).
  - App được khớp với pid bằng `GetConnectionUnixProcessID` (`native.rs:737-743`).
  - Mỗi node đọc song song `get_interfaces`, `get_role_name`, `name`, `get_state`, child count, Action, `Value.current_value` và Text (`native.rs:1659-1830`). Bounds đọc bằng `Component.GetExtents(Screen)`.
  - State đọc được Focused, Checked, Enabled, Selectable, Showing, Expanded, Selected (`native.rs:1744-1790`), nhưng chỉ `enabled` và `selected` ra output.
  - Tối đa 2048 con mỗi container (`native.rs:649`). Timeout 3 s mỗi call, 25 s mỗi thao tác (`native.rs:35,37`).
  - Cây AT-SPI là theo process. Code gắn nó vào X11 window id bằng cách khớp hình học trong khoảng 160 px (`native.rs:991`).
- **Phạm vi:** một app (pid), có thể thêm `window_id`. Không có chế độ cả desktop cho cây.
- **Field tới agent:**
  - macOS JSON: `{role, depth, element_index, element_token, label, title, description, placeholder, subrole, value, value_description, min, max, enabled, selected, actions, in_web_content, frame}` (`platform-macos/src/tools/get_window_state.rs:1187-1290`).
  - Windows có thêm `parent_index` (`platform-windows/src/tools/get_window_state.rs:60-115`). Linux tương tự Windows (`platform-linux/src/tools/get_window_state.rs:155-221`).
  - **Không có `focused` hay `expanded` trong output ở OS nào.** Trên Windows, IsOffscreen được cache nhưng không xuất.
  - `element_token` có dạng `s<8hex>:<row>`.
- **Định dạng:** `tree_format` là `markdown|elements|both`. Có `since:<snapshot_id>` để chỉ trả các dòng đổi (`cua-driver-core/src/window_state_view.rs:1-21`). Phần model thấy bị cắt còn 250 element (`window_state_view.rs:33`).

### 1.2 Hành động semantic
- **macOS:**
  - Map sang `AXPress/AXShowMenu/AXPick/AXConfirm/AXCancel/AXOpen/…` (`platform-macos/src/input/ax_actions.rs:279-287`) rồi gọi `AXUIElementPerformAction` (`tools/click.rs:1925`). Nếu element không quảng bá AXPress thì ghi `AXSelected=true` lên container chọn được (`ax_actions.rs:37-41`).
  - Click theo pixel ở nền: hit-test AX trước rồi AXPress (`click.rs:1269-1335`). Nếu không được thì theo `PixelClickRoute` (`tools/pixel_route.rs:20-49`): `BackgroundPid` dùng `SLEventPostToPid` (SkyLight, private) và `CGEventSetWindowLocation`; `ForegroundHid` dùng CGEvent HID tap.
  - `type_text` ghi `AXSelectedText`, lùi về phím CGEvent nếu không được (`tools/type_text.rs:6-20`).
  - `set_value` dùng `AXUIElementSetAttributeValue(AXValue)` (`tools/set_value.rs:9-16`).
- **Windows** (`platform-windows/src/tools/click.rs:851-990`):
  1. Với Chromium, chèn input theo tọa độ **trước tiên**, vì Invoke có thể trả thành công mà không kích hoạt gì (`:858-866`).
  2. Sau đó `Invoke` → `Toggle` → `SelectionItem.Select` → `ExpandCollapse.Expand` (`:900-955`). Mỗi lần gọi được bọc trong `EnableWindow(host, FALSE)` để host XAML/Chromium không tự kéo lên foreground (`uia/fg_bypass.rs:1-30`).
  3. Toolkit bỏ qua input giả (`input/delivery.rs:104-134`) thì dùng `CreateSyntheticPointerDevice(PT_PEN)` + `InjectSyntheticPointerInput` (`input/inject.rs:242-281`).
  4. Còn lại dùng `PostMessage(WM_LBUTTONDOWN/UP)`. Foreground dùng `SendInput`.
  - `type_text` dùng `ValuePattern.SetValue` (`tools/type_text.rs:449`). `set_value` dùng ValuePattern → RangeValue → `CB_SETCURSEL` (`tools/set_value.rs:130-330`).
  - LegacyIAccessible chỉ xuất hiện trong tool debug.
- **Linux:**
  - Click là `Action.DoAction(i)` với action đầu tiên có tên `click|activate|press|invoke|toggle|open|jump|dodefault` (`atspi/native.rs:3390-3407`).
  - Text dùng `EditableText.InsertText/SetTextContents`, giá trị dùng `Value.SetCurrentValue` (`native.rs:2944-2978, 5413`).
  - Click pixel ở nền trên X11: MPX virtual pointer qua uinput, lùi về `XSendEvent` (`tools/impl_.rs:2891-2950`).
  - Wayland: libei, compositor helper (KWin/Hyprland/GNOME), hoặc nested compositor qua `CUA_INJECT_SOCKET`.

### 1.3 Xác minh hiệu ứng (`effect`, `verified`)
- **Kết quả công khai** là `ActionResult {effect, route, delivery{mode,delivered_count}, evidence[{kind,detail}], escalation{target,reason}, summary, error, hint, next_call, fast_path}` (`cua-driver-contract/src/outputs.rs:677-717`).
  - Platform phát payload nội bộ `{path, verified, effect, evidence, escalation}`.
  - `publish_action_result` **thay** `structuredContent` bằng bản chiếu công khai (`cua-driver-core/src/tool.rs:3740-3767`).
  - **`verified` và `path` không có trong structured result công khai.** Phần text của ToolResult được giữ "for human diagnostics" (`tool.rs:3762-3765`). Trong text đó có `path` hay không: **UNVERIFIED**, vì chưa đọc formatter.
- **Quy tắc:**
  - `confirmed` bắt buộc có evidence thuộc nhóm công khai: `accessibility_readback`, `browser_readback`, `value_readback`, `window_change` (`action_record.rs:1030-1043`).
  - `confirmed` không có evidence như vậy bị hạ xuống `unverifiable` (`action_record.rs:583-587`, đã kiểm tay).
  - `screenshot_comparison`, `native_api_result`, `event_receipt` là evidence nội bộ.
- **Thuật toán:** không re-read cả cây, không hash. Mỗi action tự đọc lại đúng chỗ nó tác động:
  - **macOS click:** chỉ `verified` khi đọc lại AXSelected (poll 25 ms tới 500 ms, settle 200 ms, đọc lần hai sau 250 ms phải khớp) (`platform-macos/src/tools/click.rs:135-138, 1830-1960`). AXPress thành công bình thường là `unverifiable`. `suspected_noop` khi action không được quảng bá và không có cửa sổ mới (`click.rs:2015, 2161-2192`).
  - **macOS `type_text`:** đọc AXValue trước và sau, đạt khi `after.contains(text)` (`type_text.rs:963-995`). Read-back trong web content không bao giờ được tin.
  - **Windows `type_text`:** `confirmed` khi `value == expected && value != before`, đọc qua ValuePattern hoặc TextPattern (`platform-windows/src/tools/type_text.rs:965-1027`). Click qua UIA, PostMessage hay SendInput đều `unverifiable`.
  - **Linux:**
    - Có cửa sổ của process đích xuất hiện hoặc đóng → `window_change` → `confirmed`. Focus nhảy sang process khác → `suspected_noop` (`platform-linux/src/tools/impl_.rs:1520-1560`).
    - Click MPX được so ảnh vùng 240×240 px trước/sau, ngưỡng mỗi kênh 24, đổi ≥ 0.4% (`input/mod.rs:2626-2697`, `impl_.rs:2705`). Nhưng core hạ kết quả này về `unverifiable` nếu không có thêm `window_change`.

### 1.4 Quản lý cửa sổ và app
- **macOS:**
  - Liệt kê cửa sổ: `CGWindowListCopyWindowInfo(OnScreenOnly|ExcludeDesktopElements)` (`platform-macos/src/windows.rs:48-206`).
  - Mở app: `openApplicationAtURL:configuration:` (`apps/nsworkspace.rs:137`).
  - Đưa lên trước: `activateWithOptions` cùng private `SLPSSetFrontProcessWithOptions` (`tools/bring_to_front.rs:332-562`).
  - Menu: `AXMenuBar → AXPress/AXPick` (`tools/invoke_menu.rs:232-292`).
- **Windows:**
  - Liệt kê cửa sổ: `EnumWindows` + `DWMWA_CLOAKED` (`uia/windows_enum.rs`).
  - Mở app: `ShellExecuteExW`/`CreateProcessW`; UWP qua `IApplicationActivationManager` (`launch_uwp.rs`).
  - Focus: `AttachThreadInput` + `SetForegroundWindow` (`input/inject.rs:119-138`).
  - Menu: duyệt UIA rồi Invoke/Expand (`uia/mod.rs:232-272`).
- **Linux:**
  - Liệt kê cửa sổ: `_NET_CLIENT_LIST_STACKING` + `_NET_WM_PID` (`x11/mod.rs:6-8`).
  - Focus: `_NET_ACTIVE_WINDOW` (`input/foreground.rs:409-545`).
  - Mở app: `.desktop` `Exec=` hoặc `xdg-open` (`tools/launch_app.rs:19-48`).

### 1.5 Chờ
- `verify_state` poll mỗi 100 ms, timeout mặc định 5000 ms (tối đa 10000), cần 2 mẫu liên tiếp đạt, tối đa 8 vị từ (`cua-driver-core/src/expectation.rs:25-29`, `cua-driver-contract/src/verification.rs:15-180`). Mỗi mẫu là một lần walk mới.
- Vị từ hỗ trợ:
  - cửa sổ `exists`/`bounds`;
  - element `{role, label_contains}` với `exists`, `value_equals`, `enabled`, `selected`.
- **Không có `wait_for_text`.** Event AT-SPI chỉ dùng để theo dõi focus. Không có UIA event và AXObserver.

### 1.6 Phiên và quyền
- **Windows:**
  - Session 0 chỉ in cảnh báo "WARNING: cua-driver serve is starting in Session 0" (`cua-driver/src/serve.rs:2394`), không từ chối chạy.
  - Autostart là scheduled task `-LogonType Interactive -RunLevel Highest` chạy lúc logon (`cua-driver/src/autostart.rs:320-336`). Tên task là `cua-driver-serve` (`cua-driver/src/bundle.rs:70-76`).
  - `OpenInputDesktop` chỉ dùng để chẩn đoán: tên desktop khác "Default" thì báo Locked (Winlogon/UAC). RDP bị ngắt được phát hiện qua `WTSQuerySessionInformationW` (`platform-windows/src/diagnostics.rs:155-260`).
  - **Không có `SetThreadDesktop`, không làm việc trên Winlogon hay secure desktop.**
- **macOS:**
  - `SessionGetInfo`/`sessionHasGraphicAccess` để không crash khi bị khởi chạy từ LaunchDaemon/SSH (`platform-macos/src/session.rs:1-45`).
  - Màn khoá được phát hiện bằng `CGSessionCopyCurrentDictionary["CGSSessionScreenIsLocked"]` (`session.rs:68-95`), chỉ để giải thích vì sao cây rỗng.
  - Chạy ngoài app bundle thì **chỉ cảnh báo** "background actions will be refused" (`cua-driver/src/responsibility.rs:176-200`).
- **Linux:**
  - Thiếu `DBUS_SESSION_BUS_ADDRESS` thì thử `/run/user/<uid>/bus` của chính nó, rồi đọc `/proc/<pid>/environ` của một process thuộc phiên desktop (`xfce4-session`, `gnome-shell`, `plasmashell`, `sway`, …) (`platform-linux/src/session_bus.rs:41-130`).
  - `XAUTHORITY` lấy từ tham số `-auth` của X server (`xauth.rs:27`).
  - **`DISPLAY` không được tự tìm**, code chỉ in gợi ý (`platform-linux/src/lib.rs:215-238`).
- **Daemon:**
  - Lệnh `cua-driver <tool>` và `cua-driver call` đều **bắt buộc daemon đang chạy**, nếu không sẽ in "Start it first" và exit 1 (`cua-driver/src/cli.rs:2625-2631`).
  - Socket nằm theo `$HOME`: macOS `~/Library/Caches/<ns>/<ns>.sock`, Linux `~/.cache/<ns>/<ns>.sock`. Windows là pipe `\\.\pipe\<ns>` (`cua-driver-core/src/daemon.rs:149-168`).
  - DACL của pipe trên Windows, tức SYSTEM ở session 0 có nối vào được daemon của user hay không: **UNVERIFIED**.
- Kết luận "chỉ chạy trong phiên user đã đăng nhập": **CONFIRMED**. Màn khoá và secure desktop chỉ được phát hiện để báo lỗi, không điều khiển được.

### 1.7 Artifact
- `start_recording(output_dir, record_video)` ghi một thư mục `turn-NNNNN/` cho mỗi action (`cua-driver-core/src/recording.rs:1-12`, `recording_tools.rs:50-131`).
  - Mỗi turn: `action.json`, `before.png`, `after.png`, `click.png`, `before_state.json`, `after_state.json`, `evidence.json`.
  - Cấp session: `session.json`, `cursor.jsonl`, `recording.mp4` (ffmpeg, hoặc ScreenCaptureKit trên macOS) (`recording.rs:743-860`).
- Có `replay_trajectory` (`recording_tools.rs:381`). Tool được đăng ký trên cả ba OS.
- History mã hoá ChaCha20-Poly1305, chỉ lưu metadata (`history.rs:3-8`).

### 1.8 Screenshot
- **macOS:** `SCScreenshotManager::capture_image` với `SCContentFilter.with_window` (`platform-macos/src/capture.rs:550-606`).
- **Windows:**
  - Cửa sổ XAML/UWP dùng Windows.Graphics.Capture.
  - Cửa sổ khác dùng `PrintWindow(PW_RENDERFULLCONTENT)`. Nếu ra ảnh đen thì `BitBlt` vùng màn hình (`platform-windows/src/capture.rs:466-690`).
  - Desktop dùng `GetDC(NULL)`+`BitBlt`.
- **Linux:** X11 dùng MIT-SHM → `XGetImage`. Wayland thử lần lượt 7 backend, cuối cùng là portal (`wayland/mod.rs:1414-1474`).
- PNG, thu nhỏ bằng Lanczos3 về cạnh tối đa 1366 (macOS) hoặc 1568 (Win/Linux) (`cua-driver-core/src/image_utils.rs:271-297`). `zoom` trả crop JPEG.
- Tọa độ click là pixel ảnh cục bộ theo cửa sổ. Mỗi element có `screenshot_frame = ((x-ox)*scale, (y-oy)*scale, …)` (`cua-driver-core/src/element_frame.rs:39-50`).

### 1.9 Bảng claim cũ
| Claim cũ | Kết luận |
|---|---|
| Cây a11y native trên 3 OS, có `element_token` | **CONFIRMED** |
| Tool MCP `get_window_state, get_desktop_state, parse_visual_regions, click, type_text, press_key, hotkey, scroll, drag, move_cursor, start_session, end_session, list_apps, list_windows, zoom, set_value` | **CONFIRMED**. Còn nhiều tool khác: `verify_state`, `start_recording`, `replay_trajectory`, `launch_app`, `browser_*`… (`cua-driver-core/src/tool.rs:900-1010`) |
| "Không có tool `screenshot` riêng" | **CONFIRMED** (`tool.rs:933-936`) |
| `cua-driver <tool> '<json>'` / `cua-driver call` | **CONFIRMED** cú pháp (`cli.rs:12-13`). Cả hai cần daemon đang chạy (`cli.rs:2625-2631`) |
| Mỗi phản hồi có `effect` 5 giá trị và `verified` | **REFUTED một phần**: `effect` công khai, còn `verified` chỉ là field nội bộ, bị bỏ khỏi structured result (`tool.rs:3740-3767`) |
| "Only accessibility read-back sets `verified` to true" | **REFUTED một phần**: `confirmed` công khai còn nhận `value_readback`, `window_change`, `browser_readback` (`action_record.rs:1030-1043`). Linux đặt `verified` từ `window_change` |
| JSON có `path` (`ax`, `cgevent`, `x11_atspi`, `msaa`) | **REFUTED cho structured result**: các giá trị này có trong code (`action_record.rs:759-830`) nhưng được đổi thành `route` và bỏ khỏi bản công khai |
| Bốn nấc cố định a11y → pixel nền → CDP → foreground | **REFUTED**: không có thang cố định. `escalation.target ∈ {Pixel, Foreground, Page, Session}` là gợi ý theo từng kết quả (`outputs.rs:651-656`). Trên Windows với Chromium, pixel đi **trước** UIA (`click.rs:858-866`). CDP là họ tool `browser_*` riêng |
| Wayland "raw background keys are refused" | **CONFIRMED**, trừ khi có `CUA_INJECT_SOCKET` (`platform-linux/src/tools/impl_.rs:2479-2491`) |
| Extension perception AGPL-3.0 | **REFUTED cho code, CONFIRMED cho model**: crate `cua-perception` mang MIT qua workspace. AGPL chỉ áp vào weights detector OmniParser (`cua-perception/models/model-manifest.template.json:8`: "AGPL-3.0 (detector); Apache-2.0 (OCR)") |
| "Không có recording hay trajectory dành cho agent" | **REFUTED**: có `start_recording`/`stop_recording`/`replay_trajectory` (§1.7) |
| Linux tự tìm `/run/user/<uid>/bus` khi thiếu `DBUS_SESSION_BUS_ADDRESS` | **CONFIRMED**, kèm cách đọc `/proc/<pid>/environ` (`session_bus.rs:41-130`) |
| Windows cần Session 1+, `schtasks /Run /TN cua-driver-serve` | **CONFIRMED** tên task (`bundle.rs:70-76`). Session 0 chỉ bị cảnh báo, không bị chặn |
| macOS: quyền thuộc `CuaDriver.app`; `serve` thô "unsupported" | **REFUTED một phần**: code chỉ cảnh báo, action nền sẽ bị từ chối, nhưng không chặn khởi chạy (`responsibility.rs:176-200`) |
| License driver MIT | **CONFIRMED** |

---

## 2. agent-desktop (`vercel-labs/agent-desktop`)

Tiền tố: W: = `crates/windows/src/`, M: = `crates/macos/src/`, L: = `crates/linux/src/`, C: = `crates/core/src/`. Workspace version `1.3.0` (`Cargo.toml:10`).

### 2.1 Cây accessibility
- **Chung:**
  - Mọi lệnh in một envelope `{version:"2.6", ok, command, data?, error?}` (`C:response/output.rs:7-18`, đã kiểm tay).
  - Snapshot trả `{app, ref_count, snapshot_id, complete, tree, window{id,title}}` (`C:commands/snapshot.rs:136-143`). `tree` **là cây lồng nhau**. Mỗi node có `ref_id, role, name, value, description, native_id, hint, states[], available_actions[], bounds?, children_count, subtree_truncated, children[]` (`C:tree/node.rs:8-31`).
  - Ref có dạng `@<snapshot_id>:eN` (`C:refs/ref_token.rs:5`).
  - Cờ: `--max-depth` (mặc định 10), `-i` (chỉ element tương tác), `--compact` (gộp node một-con không tên), `--skeleton` (depth logic 3, ghi `children_count` ở chỗ cắt), `--include-bounds`, `--timeout-ms` (mặc định 3000) (`src/cli_args/snapshot_tree.rs:11-31`, `C:snapshot/mod.rs:162`).
  - Phạm vi mặc định là **cửa sổ đang focus**. `--app` hoặc `--window-id` chọn một cửa sổ khác. Không bao giờ walk cả desktop (`C:snapshot/surface_target.rs:160-162`).
  - Ngân sách: depth thô 50, 50 000 node, 100 000 cạnh, 4 096 con mỗi node (`C:locator/live_locator/observation_budget.rs:3-5`).
- **Windows:**
  - `CoCreateInstance(CUIAutomation8)`, ConnectionTimeout 2000 ms, TransactionTimeout 20000 ms (`W:tree/automation.rs:30,33,190-203`).
  - Trước khi đụng vào cửa sổ, probe treo bằng `SendMessageTimeoutW(WM_NULL, SMTO_ABORTIFHUNG)`.
  - **Không dùng FindAllBuildCache**: đi Raw-view TreeWalker bằng `GetFirstChildBuildCache`/`GetNextSiblingBuildCache` (`W:tree/walk/walker_source.rs:171-232`).
  - Cache 43 property (`W:tree/properties/property_ids.rs:135-178`):
    - Name, AutomationId, Value;
    - LegacyIAccessible State/DefaultAction;
    - IsEnabled, HasKeyboardFocus, IsPassword, IsOffscreen;
    - ToggleState, ExpandCollapseState, SelectionItem.IsSelected, IsReadOnly, Window.IsModal;
    - mọi `Is*PatternAvailable`.
  - Bỏ cache khi provider chỉ là proxy `uiautomationcore.dll` (`W:tree/walk/cache.rs:42-88`).
  - State xuất ra: disabled, focused, checked, expanded, selected, readonly, modal, offscreen, pressed, haspopup, busy (`W:tree/states.rs:70-181`).
- **macOS:**
  - Một `AXUIElementCopyMultipleAttributeValues` mỗi node với 23 attribute, gồm AXEnabled, AXFocused, AXExpanded, AXSelected, AXModal, AXIdentifier, AXDOMIdentifier (`M:tree/properties/node_attribute_names.rs:17-42`).
  - AXValue đọc riêng và không bao giờ đọc cho `AXSecureTextField`.
  - Messaging timeout tối đa 250 ms mỗi lần đọc (`M:tree/properties/ax_ipc.rs:20,44-57`).
  - Bật `AXManualAccessibility`/`AXEnhancedUserInterface` cho Chromium.
  - Ref dùng hàm private `_AXUIElementRemoteTokenCreate` qua `dlsym` (`M:tree/resolve/retained.rs:99-100`).
- **Linux:**
  - Session bus → `org.a11y.Bus.GetAddress` → kết nối riêng tới bus a11y (`L:system/bus/mod.rs:48-84`). Sau đó gọi `Registry.RegisterEvent("window:activate")` vì Chromium chỉ báo cửa sổ active khi có listener (`:94-110`).
  - Đọc song song, 32 request cùng lúc (`L:tree/walk/walker_expand.rs:18`). Không dùng `Cache.GetItems`.
  - Giới hạn: depth 50, 10 000 node (`L:tree/walk/walk_limits.rs:3-7`).

### 2.2 Hành động semantic
- **Windows** (`W:actions/dispatch.rs:192-260`):
  - Click: `Invoke` → `LegacyIAccessible.DoDefaultAction`. **Click không có fallback vật lý.**
  - Toggle; ExpandCollapse; `Value.SetValue` → `RangeValue.SetValue`; `SelectionItem.Select`; `ScrollItem.ScrollIntoView`; `Scroll`.
  - `SetFocus` bị từ chối nếu không có `--headed`.
  - `SendInput` chỉ cho double/right click, type, press, và cửa sổ phải đã ở foreground (`W:actions/physical_target.rs:37-75`).
- **macOS:**
  - Chuỗi click: `AXPress` → `AXOpen` → … → `AXConfirm` (`M:actions/chain/chain_defs.rs:12-35`).
  - `AXShowMenu`; ghi AXValue hoặc `AXIncrement`/`AXDecrement`.
  - Gõ không cần focus bằng cách ghi `AXSelectedText` (`M:actions/type_text.rs:26-55`).
  - Input vật lý: `CGEventPost` cho chuột, `CGEventPostToPid` cho phím.
- **Linux:** `Action.DoAction` chọn theo tên, `EditableText.SetTextContents`, `Value.CurrentValue`, `Selection.SelectChild`, `Component.GrabFocus` (`L:actions/action_call.rs:9-38`). Input vật lý: XTest, hoặc `org.gnome.Mutter.RemoteDesktop` trên Wayland (`L:input/wayland/remote_desktop.rs:13-18`).
- **Headless mặc định:** policy `{allow_focus_steal:false, allow_cursor_move:false}` (`C:context/mod.rs:110-115`). Không có đường semantic thì trả `POLICY_DENIED` (`C:actionability/gates.rs:151-177`).

### 2.3 Xác minh hiệu ứng
- `error.disposition = {delivery ∈ unknown|not_delivered|delivery_uncertain|delivered_unverified|delivered_verified, retry}`. `retry` chỉ `safe` khi `not_delivered` (`C:actions/delivery_semantics.rs:47-66`).
- Read-back theo OS:
  - **Windows:** đọc lại Value; poll ToggleState 600 ms; ExpandCollapse 200 ms; Select 600 ms; so focus bằng `GetFocusedElement`. Invoke và `SendInput` luôn là unverified (`W:actions/dispatch.rs:203-206`).
  - **macOS:** đọc focus, selection và menu trước/sau, settle 40/400 ms (`M:actions/activation_effect.rs:7-8,18-69`).
  - **Linux:** đọc lại text; poll FOCUSED 500 ms; toggle 1 s.
- Không so ảnh.

### 2.4 Cửa sổ và app
- **Windows:**
  - Liệt kê: `EnumWindows` lọc bỏ cloaked và tool window (`W:system/window/window_enum.rs:61-94`).
  - Mở app: `CreateProcessW` **không** đặt `lpDesktop` và không dùng token (`W:system/launch.rs:221`).
  - Focus: `ShowWindowAsync` + `SetForegroundWindow` (`W:system/window/window_activate.rs:228-270`).
- **macOS:**
  - Liệt kê: `NSWorkspace.runningApplications` và `CGWindowListCopyWindowInfo`.
  - Mở app: `openApplicationAtURL:configuration:` (`M:system/launch_bridge.m:140-163`).
  - Focus: `activateWithOptions:` + `AXRaise` (`M:system/focus.rs:182-203`).
- **Linux:**
  - Liệt kê: frame AT-SPI, đối chiếu với `_NET_CLIENT_LIST`.
  - Focus: `_NET_ACTIVE_WINDOW`, chỉ X11; Wayland bị từ chối.
  - Mở app: dòng `Exec` của file `.desktop`.

### 2.5 Chờ
- `wait` hỗ trợ:
  - ngủ một số ms;
  - `--element <ref> --predicate exists|enabled|visible|actionable|value`;
  - `--window`, `--text`, `--menu`, `--notification`;
  - `--event window-opened|…|focus-changed` (`C:commands/wait_mode.rs:7-37`).
- Timeout mặc định 30 s. Poll mỗi 100 ms (`C:commands/wait_scheduler.rs:5-35`).
- Chỉ Linux đăng ký event AT-SPI để đánh thức vòng poll sớm (`L:system/signal/subscription.rs:177-260`). Windows và macOS chỉ poll.
- Có thêm `--wait-for`/`--wait-for-gone` sau action (`src/cli/post_action_wait.rs:5-25`).

### 2.6 Phiên và quyền
- **Windows:**
  - Không có code nào về session 0, input desktop, Winlogon hay UAC. Duy nhất có gợi ý trong permissions probe khi `CoCreateInstance` trả `E_ACCESSDENIED` (`W:system/permissions.rs:3,20-41`).
  - Có kiểm UIPI: so `TokenIntegrityLevel`; đích cao hơn thì trả `PERM_DENIED` (`W:input/elevation.rs:63-165`).
- **macOS:**
  - TCC tính cho app khởi chạy; guide đi ngược process cha tới `.app` ngoài cùng (`M:system/permissions/permission_guide.rs:130-134`).
  - Binary `agent-desktop-macos-helper` chỉ lo clipboard (`M:bin/agent-desktop-macos-helper.rs:1-3`).
  - Không xử lý root, launchd hay màn khoá.
- **Linux:**
  - Lỗi bus được phân loại `NoSessionBus|NotInstalled|CannotStart|SocketGone` (`L:system/bus/bus_fault.rs:12-73`).
  - **Không tự tìm `/run/user/<uid>/bus`**, không bật `org.a11y.Status`.
- Kết luận "chỉ trong phiên user": **CONFIRMED**. Đây là yêu cầu ngầm: không OS nào có code phát hiện hay xử lý việc chạy ngoài phiên user. Ở SYSTEM/root, nó sẽ làm việc trong session của chính nó; hành vi cụ thể **UNVERIFIED**.

### 2.7 Artifact
- State root: `~/.agent-desktop` (`C:storage/state_root.rs:4-111`).
- Session có `sessions/<id>/session.json` và trace JSONL, tối đa 32 MiB. `--trace <path>` ghi JSONL vào file chỉ định.
- Ảnh mỗi bước chỉ có khi session bật `--artifacts`: `trace/screens/<pid>-<ms>-<seq>-{pre|post}.png` và `trace/refmaps/<snapshot_id>.json`. Giới hạn 128 MiB hoặc 200 ảnh (`C:trace/trace_artifacts.rs:47-146`, `trace_artifact_budget.rs:8-10`).
- Không có video.

### 2.8 Screenshot
- Có PATH thì ghi file, không thì trả base64. Kèm `{format, width, height, scale_factor}`. **Không resize** (`C:commands/screenshot.rs:28-41`).
- **Windows:** Windows.Graphics.Capture, lùi về `BitBlt`/`PrintWindow(PW_RENDERFULLCONTENT)`, mã hoá PNG qua WIC. `SendInput` dùng tọa độ 0..65535 với `MOUSEEVENTF_VIRTUALDESK` (`W:input/mouse/mouse_coord.rs:21-70`).
- **macOS:** gọi `/usr/sbin/screencapture -x -t png [-D n | -l wid -o]` (`M:system/screenshot.rs:17,40-64`).
- **Linux:** X11 `GetImage`; Wayland `org.freedesktop.portal.Screenshot`.

### 2.9 Bảng claim cũ
| Claim cũ | Kết luận |
|---|---|
| Apache-2.0 | **CONFIRMED** |
| macOS 13+, Windows 10 1809+, Ubuntu 24.04+ | **REFUTED một phần**: không có kiểm tra lúc chạy. 1809 chỉ có trong comment (`W:tree/automation.rs:186-187`). macOS build mặc định target 10.15 (`crates/macos/build.rs:206`). Wayland chỉ có trên GNOME/Mutter |
| 61 lệnh, có `snapshot -i --compact --skeleton`, `wait`, `batch`, … và `mcp` | **CONFIRMED** (`src/cli/mod.rs:194-256`). `mcp` nằm ngoài 61 lệnh (`src/main.rs:88-89`) |
| Envelope `{version, ok, command, data\|error}`, `error.disposition.retry`, `STALE_REF`, `AMBIGUOUS_TARGET` | **CONFIRMED** |
| "without screenshots or pixel matching" | **CONFIRMED** cho quan sát và action. Vẫn có lệnh `screenshot` |
| Ref action "headless by default" | **CONFIRMED** |
| `screenshot` lưu file hay base64 (trước là UNVERIFIED) | Cả hai, tùy có PATH hay không (`C:commands/screenshot.rs:28-41`) |
| Windows session 0, secure desktop, màn khoá "not supported" | **CONFIRMED**: chỉ ghi trong docs (`skills/agent-desktop-windows/SKILL.md:22-25`), code không phát hiện |
| Linux cần session bus và a11y bus; macOS TCC theo app cha | **CONFIRMED** |

---

## 3. Peekaboo (`openclaw/Peekaboo`) và AXorcist (`openclaw/AXorcist`)

Tiền tố PB: = `Peekaboo/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/`. CLI: = `Peekaboo/Apps/CLI/Sources/PeekabooCLI/`. AX: = `AXorcist/Sources/`.

### 3.1 Cây accessibility
- Đường đi: `see` → `ElementDetectionService` → `DetachedAXObservationWorker` (`PB:Services/UI/ElementDetectionService.swift:147-154`).
- **Gốc:** `AXUIElementCreateApplication(pid)`, rồi chỉ walk **một cửa sổ**, cửa sổ đã chụp (`PB:Services/UI/DetachedAXObservationWorker.swift:260`). Comment ghi lý do: "Walking the app root also visits sibling windows" (`PB:Services/UI/AXTreeCollector.swift:83-85`).
- **Phạm vi:**
  - `--screen-index` cho cả màn hình, `--app/--pid/--window-*` cho một cửa sổ, mặc định là frontmost (`CLI:Commands/AI/SeeCommand+ObservationRequest.swift:11-22`).
  - Ở chế độ screen, phần AX vẫn chỉ lấy app frontmost (`PB:Services/UI/ElementDetectionWindowResolver.swift:88-94`).
- **Lời gọi và attribute:**
  - Mỗi node một `AXUIElementCopyMultipleAttributeValues` cho 16 attribute: AXPosition, AXSize, AXRole, AXTitle, AXLabel, AXValue, AXDescription, AXHelp, AXRoleDescription, AXIdentifier, AXEnabled, AXSelected, AXFocused, AXPlaceholderValue, AXEditable, AXKeyboardShortcut (`DetachedAXObservationWorker.swift:109-125, 697`).
  - Thêm một lần gọi nữa cho 15 attribute con (AXChildren, AXVisibleChildren, AXContents, AXRows, AXTabs…) (`:92-107, 785`).
  - Lô thất bại thì lùi về `AXUIElementCopyAttributeValue` từng cái.
  - `AXUIElementCopyActionNames` chỉ gọi cho một số role. `AXUIElementIsAttributeSettable(kAXValue)` gọi cho role dạng text (`:823-850`).
  - AXExpanded không được đọc.
- **Giới hạn:**
  - depth 12, 1000 element, 250 con mỗi node; đổi được qua `--depth`, `--max-elements`, `--max-children` hoặc env (`PB:Services/Observation/DesktopObservationRequestModels.swift:35-41`);
  - `AXUIElementSetMessagingTimeout(min(0.2 s, thời gian còn))` trước mỗi node (`DetachedAXObservationWorker.swift:91, 852-868`);
  - tổng 20 s (`ElementDetectionService.swift:272-275`).
  - Chạm giới hạn thì ghi cảnh báo `ax_truncated_*` (`ElementDetectionResultBuilder.swift:19-37`).
- **Output `see --json`:**
  - **Danh sách phẳng**, không có quan hệ cha-con. Code ghi: "Parent-child relationships are not directly supported" (`ElementDetectionService.swift:256`).
  - Mỗi dòng `ui_elements[]`: `id (elem_N), role, ax_role, title, label, value, description, role_description, help, identifier, bounds (global points), is_actionable, is_enabled, is_selected, is_value_settable, keyboard_shortcut, selected_text_range` (`Peekaboo/Core/PeekabooCore/Sources/PeekabooAgentRuntime/Support/UIElementSummary.swift:9-27`).
  - Kèm `snapshot_id`, `screenshot_raw`, `screenshot_annotated`, `ui_map` (đường dẫn tới `snapshot.json`), `coordinate_context` (`CLI:Commands/AI/SeeCommand+Types.swift:350-372`).
  - `--tree` in text phẳng, mỗi element một dòng (`SeeCommand+Output.swift:237-252`).

### 3.2 Hành động semantic
- AX action dùng trong code:
  - `AXPress` (`PB:Services/UI/ActionInputDriver.swift:175`);
  - `AXShowMenu` (`:224`), chạy trên thread tách riêng với 0.5 s chờ vì nó block trong vòng lặp tracking của menu (`PB:Services/UI/DetachedAXActionRunner.swift:8-25`);
  - `AXScrollUpByPage`…, `AXIncrement`/`AXDecrement` (`ActionInputDriver.swift:944-953`), `AXHide`.
- `AXUIElementSetAttributeValue` cho AXValue, AXFocused, AXSelected (`PB:Services/UI/AutomationElement.swift:328-350`).
- `set-value` đổi giá trị đúng kiểu (bool, số, chuỗi). Nếu AXValue không ghi được thì ghi AXSelected. Từ chối `AXSecureTextField` (`ActionInputDriver.swift:434-447, 625-670, 891-940`).
- Chiến lược (`PB:Strategy/UIInputPolicy.swift:52-56, 79, 106`): click/scroll là `actionFirst`, setValue/performAction là `actionOnly`, các thao tác khác `synthFirst`.
- **"Background delivery":**
  - Là mặc định khi đích là process hoặc cửa sổ cụ thể: `deliveryIntent = target == .foreground ? .foreground : .background` (`PB:Strategy/DesktopOperationPlan.swift:162`, đã kiểm tay).
  - Phím đi qua `SLEventPostToPid` (SkyLight private, `dlsym`), lùi về `CGEvent.postToPid` (`PB:Services/UI/BackgroundInputDriver.swift:348-352, 1653-1676`).
  - Click pixel nền dùng `CGEventSetWindowLocation` (private) và không di con trỏ (`PB:Services/UI/WindowRoutedPointerDriver.swift:13-17, 931`).

### 3.3 Xác minh hiệu ứng
- `set-value`, ghi AXSelected hoặc AXFocused: đọc trước; nếu đã đúng thì trả `confirmedNoChange`. Nếu chưa, đọc lại tối đa 14 lần trong 250 ms, cách nhau 20 ms; không khớp thì báo lỗi `indeterminate` (`ActionInputDriver.swift:612-740, 816-880`).
- `AXPress` và action khác trả `.dispatchedUnverified` (`:536-543`).
- Gõ chữ: read-back 250 ms, poll 20 ms (`PB:Services/UI/ExactLiteralTypingEffectConfirmation.swift:2-7`).
- Lệnh `verify` / MCP `verify_state` (`CLI:Commands/AI/VerifyCommand.swift:3-14`):
  - vị từ window-exists, window-bounds, element exists, value-equals, enabled, selected;
  - poll 100 ms, cần 2 mẫu ổn định, mặc định 5 s, tối đa 10 s.
- So ảnh chỉ dùng cho watch/capture (`PB:Services/Capture/WatchFrameDiffer.swift:49-182`), không dùng để xác minh action.

### 3.4 Cửa sổ, app, menu, dock, dialog
- Cửa sổ: `CGWindowListCopyWindowInfo` cùng `_AXUIElementGetWindow` (`PB:Services/System/ApplicationWindowEnumerationContext.swift:144-150`). Focus bằng `activate()`/AXFrontmost → AXMain → `AXRaise` (`PB:Utilities/FocusUtilities.swift:1040-1047, 1206-1223`).
- App: `NSWorkspace.openApplication`, `terminate`, AXHide (`PB:Services/System/ApplicationService.swift:215-281`).
- Menu: `AXMenuBar` → AXPress từng mục (`PB:Services/UI/MenuService+Actions.swift:220`).
- Dock: cây AX của `com.apple.dock` (`PB:Services/UI/DockService+Items.swift:13`).
- Dialog: phân loại theo subrole AXDialog/AXSheet/AXSystemDialog và AXModal (`PB:Services/UI/DialogElementClassification.swift:84-122`).

### 3.5 Chờ
- `click --wait-for` poll 100 ms, mặc định 5 s (`PB:Services/UI/UIAutomationService+ElementLookup.swift:201-235`). Chờ điều kiện tổng quát dùng `verify` (§3.3).
- AXObserver chỉ được `WindowTrackerService` của daemon dùng để theo dõi cửa sổ (`PB:Services/Support/WindowTrackerService.swift:33, 123-145`), không dùng để chờ action.

### 3.6 Phiên và quyền
- **TCC:**
  - Screen Recording qua `CGPreflightScreenCaptureAccess` và probe `SCShareableContent` (`PB:Services/Capture/ScreenCapturePermissionGate.swift:65-118`);
  - Accessibility qua `AXIsProcessTrustedWithOptions` (`AX:AXorcist/Core/AXPermissionHelpers.swift:50`);
  - "Event Synthesizing" qua `CGPreflightPostEventAccess`/`CGRequestPostEventAccess` (`PB:Services/System/PermissionsService.swift:28-33, 211-238`). **Không phải Input Monitoring.**
- **Ảnh chỉ có wallpaper:** không có kiểm tra pixel. Code suy ra từ metadata: có cửa sổ layer 0 của process khác mang `kCGWindowName` không rỗng hay không (`ScreenCapturePermissionGate.swift:121-149`).
- **Màn khoá:** `CGSessionCopyCurrentDictionary()["CGSSessionScreenIsLocked"]` → lỗi "Screen capture is unavailable while the macOS GUI session is locked" (`PB:Services/System/DesktopSessionLockState.swift:4-13`, đã kiểm tay; `PB:Services/Capture/ScreenCaptureService+Operations.swift:68-72`). Không có code cho loginwindow.
- **Bridge:**
  - Socket nằm dưới `~/Library/Application Support/<App>/…/bridge.sock`, tính từ home của **người gọi** (`Peekaboo/Core/PeekabooCore/Sources/PeekabooBridge/PeekabooBridgeConstants.swift:17-40, 255-261`). Socket mode 0600.
  - Host kiểm peer bằng `LOCAL_PEERTOKEN` và Team ID, hoặc cùng UID khi allowlist rỗng (`PeekabooBridgeHost+Clients.swift:238-314`).
  - **Không có code nào nhận biết SSH, launchd hay root.** Từ root, path sẽ ra `/var/root/...` trừ khi truyền `--bridge-socket`. Root có vào được socket 0600 của user hay không, và TCC nào áp dụng: **UNVERIFIED**.
- Kết luận "chỉ trong phiên user": **CONFIRMED theo thiết kế**: path và TCC theo user, chụp ảnh bị chặn khi khoá. Code không có guard rõ ràng cho việc này.

### 3.7 Artifact
- `~/.peekaboo/snapshots/<id>/` chứa `snapshot.json`, `raw.png`, `annotated.png` (`PB:Services/Support/SnapshotManager+Helpers.swift:122-133`).
- Ảnh `see` mặc định ở `~/Desktop/peekaboo_see_<unix>.png` (`CLI:Commands/AI/SeeCommand+CaptureSupport.swift:49-63`).
- Video và watch nằm ở `$TMPDIR/peekaboo/capture-sessions/capture-<UUID>/` gồm `keep-%04d.png`, `contact.png`, `capture.mp4` (AVAssetWriter) (`PB:Services/Capture/VideoWriter.swift:72-73`).
- **Không tự động chụp ảnh mỗi bước action.**
- Receipt ký của Bridge ở `$TMPDIR/PeekabooOperationReceipts/…`.

### 3.8 Screenshot
- Engine "modern" là `SCScreenshotManager.captureImage` (`PB:Services/Capture/ScreenCaptureKitCaptureGate.swift:144-158`).
- Engine "legacy" thực ra gọi `/usr/sbin/screencapture -x -D/-l` (`PB:Services/Capture/LegacyScreenCaptureOperator+SystemScreencapture.swift:112-150`) và **cố ý từ chối** `CGDisplayCreateImage` (`…+ScreenArea.swift:43-46`).
- Mặc định 1x logical: ảnh native bị chia theo `backingScaleFactor`. `--retina` giữ native (`PB:Services/Capture/ScreenCaptureImageScaler.swift:5-30`).
- PNG mặc định; JPG chất lượng 0.92.
- Đổi tọa độ ngược: `x = bounds.minX + px / imageW * bounds.width` (`PB:Services/Core/CaptureCoordinateMapper.swift:37-79`). AX bounds trong `ui_elements` đã là global points.

### 3.9 Bảng claim cũ
| Claim cũ | Kết luận |
|---|---|
| Peekaboo MIT, macOS 15+ | MIT **CONFIRMED**. macOS 15+ **REFUTED một phần**: chỉ package CLI là `.v15` (`Peekaboo/Apps/CLI/Package.swift:150`); root, Core và Mac là `.v14` (`Peekaboo/Package.swift:22`) |
| AXorcist MIT, macOS 14+ | **CONFIRMED** (`AXorcist/Package.swift:14`) |
| `see --app Finder --json` trả "structured UI map with opaque element IDs" | **CONFIRMED** nhưng map là **phẳng**, id `elem_N` chỉ có nghĩa trong một snapshot |
| `see --mode screen --path …` | **CONFIRMED**. Phần AX vẫn chỉ lấy app frontmost |
| Lệnh click/type/press/scroll/drag/set-value/action/window list/app/menu/dock/dialog | **CONFIRMED**. `hotkey` đã gộp vào `press` |
| "Background delivery" khi resolve được process | **CONFIRMED**: mặc định, qua `SLEventPostToPid`/`postToPid`/AX action |
| Quyền thứ ba cho synthetic input | **CONFIRMED**: PostEvent (`CGPreflightPostEventAccess`), không phải Input Monitoring |
| Bridge dành cho SSH/LaunchAgent/Codex | **NOT FOUND** trong code: chỉ có trong docs. Code không nhận biết SSH hay launchd |
| `axorc tree --depth 3`, `find --role --title`, `raw --stdin` + `performAction` | **CONFIRMED** (`AXorcist/Sources/axorc/CLIFrontend.swift:157-325`, `AXorcist/Sources/AXorcist/Core/CommandTypes.swift:20`) |
| `axorc` lọc theo AXValue, AXEnabled, AXFocused | **REFUTED một phần**: `find` chỉ có `--value`. AXEnabled/AXFocused chỉ có qua locator JSON trong `raw` (`AXorcist/Sources/AXorcist/Search/SpecificAttributeMatchers.swift:162-163`) |
| Peekaboo có MCP | **CONFIRMED**: 27 tool, gồm `see`, `click`, `set_value`, `verify_state`, `dialog`, `menu`… (`Peekaboo/Core/PeekabooCore/Sources/PeekabooAgentRuntime/MCP/Server/MCPToolCatalog.swift:95-129`) |

---

## 4. Windows-MCP (`CursorTouch/Windows-MCP`)

Tiền tố S: = `src/windows_mcp/`. Thư viện `uiautomation` của yinkaisheng được **vendored và đã sửa** dưới `S:uia/` (`S:uia/core.py:1-11`), không phải dependency.

### 4.1 Cây accessibility
- **Client:** `CUIAutomation8`/`IUIAutomation2` với ConnectionTimeout 2000 ms, TransactionTimeout 5000 ms (`S:uia/core.py:52-53, 114-127`). Mọi việc UIA chạy trên một worker thread duy nhất (`S:infrastructure/thread_affinity.py:27`).
- **Phạm vi:**
  - `EnumWindows` (visible, cùng virtual desktop) cộng `Progman`, `Shell_TrayWnd`, `Shell_SecondaryTrayWnd` (`S:desktop/service.py:1413-1438`).
  - Chỉ walk **cửa sổ foreground cộng mọi top-level không phải "app window"**: taskbar, popup, context menu, dialog không có nút min/max (`S:desktop/service.py:230, 1554-1562`). App ở nền không được walk, trừ khi truyền `region=`.
- **Cách walk:**
  - Mỗi cửa sổ: `ControlFromHandle`, rồi `BuildUpdatedCache(TreeScope_Children)` → `GetCachedChildren` từng tầng (`S:tree/cache_utils.py:147, 177-210`).
  - DFS **không giới hạn depth**. Chặn vòng lặp bằng RuntimeId.
  - Cache khoảng 35 property (`S:tree/cache_utils.py:57-125`): Name, AutomationId, ControlType, IsEnabled, IsOffscreen, HasKeyboardFocus, IsPassword, BoundingRectangle, LegacyIAccessible Role/Value/DefaultAction/State, ToggleState, ExpandCollapseState, SelectionItem.IsSelected, RangeValue, Window.IsModal, Scroll percent.
- **Lọc:**
  - Phải có diện tích > 0, on-screen, IsControlElement, **IsEnabled**. Element bị disable bị loại khỏi cây (`S:tree/service.py:588-592`).
  - Có allowlist control type × LegacyIAccessible role (`S:tree/config.py:1-63`).
  - Gặp dialog `IsModal` thì xoá hết và chỉ giữ dialog (`S:tree/service.py:917-927`).
- **Giới hạn:** ngân sách 500 element (`S:tree/budget.py:19`).
- **Field tới agent:**
  - **Text, không phải JSON**: cây ASCII dạng `├── (x,y) button "OK"  [action: click]  [focused]  [toggle:on]` (`S:tree/views.py:150-178, 35-75`).
  - Chỉ có tâm `(x,y)`, **không in bbox**.
  - Metadata gồm focused, toggle_state, value (LegacyIAccessible), is_selected, expand_collapse_state, slider min/max/value (`S:tree/service.py:655-797`).
  - **Label số không có trong text**, chỉ được vẽ lên ảnh annotate (`S:desktop/service.py:734-746, 1792-1830`).
- `use_vision` gắn ảnh PNG có annotate. `use_dom=True` chỉ giữ cây dưới `RootWebArea` của Chrome/Edge/Firefox (`S:tree/service.py:900-907`).

### 4.2 Hành động semantic
- **Không có.** Pattern duy nhất được gọi là `ValuePattern.SetValue("")` để xoá ô nhập (`S:desktop/service.py:1060-1071`).
- Click: `SetCursorPos` + `mouse_event(…ABSOLUTE|VIRTUALDESK)` (`S:uia/core.py:276-347`).
- Phím: chữ gõ bằng `SendInput KEYEVENTF_UNICODE` (`:1446-1487`); phím tắt bằng `keybd_event`. Chuỗi từ 20 ký tự trở lên thì dán qua clipboard (`S:desktop/service.py:855, 1116-1146`).

### 4.3 Xác minh hiệu ứng
- `Type` đọc lại `ValuePattern.Value`/`WM_GETTEXT` và cảnh báo nếu focus đã đổi (`S:desktop/service.py:1016-1031, 1078-1114`).
- `Click` kiểm modal chặn trước khi click: `WindowFromPoint` → `GA_ROOT` → `IsWindowEnabled` (`:794-805`).
- `App switch` poll `GetForegroundWindow` 1 s.
- `ControlStatus` **không** xác minh hiệu ứng. Nó báo ai đang giữ desktop (AI hay người), dùng low-level hook `WH_MOUSE_LL`/`WH_KEYBOARD_LL` và bỏ qua event có cờ `LLMHF_INJECTED` (`S:desktop/control.py:684-685`, `S:desktop/control_hooks.py:27,75`).

### 4.4 Cửa sổ và app
- `App launch`: `Get-StartApps` (PowerShell) hoặc quét `.lnk`, so mờ tên, rồi `Start-Process` hoặc `shell:AppsFolder\<AppID>` (`S:desktop/service.py:398-468, 590-626`).
- `App switch`: `ShowWindow(SW_RESTORE)` → `AllowSetForegroundWindow(-1)` → `AttachThreadInput` → `SetForegroundWindow`/`BringWindowToTop`/`SetWindowPos(HWND_TOP)` (`:628-732`).
- `resize`: `MoveWindow` qua UIA.

### 4.5 Chờ
- `Wait` là `time.sleep` (`S:tools/input.py:440-443`).
- `WaitFor` poll lại toàn bộ `get_state` mỗi 0.25 s (tối đa 5 s), timeout 10 s (tối đa 120 s), với điều kiện `text_exists|active_window|element_exists|element_enabled|focused_element` (`S:tools/input.py:16-22, 166-212, 466-511`).
- Không dùng UIA event để chờ.

### 4.6 Phiên và quyền
- Không có code cho SYSTEM, session 0, Winlogon, màn khoá hay UAC.
- Có guard `_interactive_desktop()`: `WTSConnectState == WTSActive` **và** tên input desktop phải là `"Default"`. Comment: "The input desktop changes to Winlogon for UAC/lock screen" (`S:desktop/control_win32.py:221-252`, đã kiểm tay).
  - Lúc khởi động mà guard sai thì server không chạy (`S:desktop/control.py:472-473`, `S:__main__.py:364-368`).
  - Khi đang chạy thì trạng thái thành `unavailable` và tool input trả `CONTROL_UNAVAILABLE` (`S:desktop/control_worker.py:63-77`).
- `windows-mcp install` dùng `Register-ScheduledTask` với trigger `-AtLogOn -User $env:USERNAME` và `-RunLevel Limited` (`S:__main__.py:1035-1102`, đã kiểm tay). Logon type không được đặt nên là mặc định của Task Scheduler: **UNVERIFIED**.
- Kết luận "chỉ trong phiên user": **CONFIRMED**. Suy luận cũ "không thấy màn khoá hay UAC" giờ **CONFIRMED và mạnh hơn**: nó phát hiện các màn đó rồi tắt điều khiển.

### 4.7 Artifact và telemetry
- Không có ảnh mỗi bước, recording hay trajectory. PNG chỉ ghi khi có `save_path` (`S:desktop/screenshot_file.py:8-32`). Log nằm ở `~/.windows-mcp/server.log` khi cài bằng task.
- **Telemetry bật mặc định:** `os.getenv("ANONYMIZED_TELEMETRY", "true")` (`S:__main__.py:349`, đã kiểm tay).
  - Gửi lên PostHog `us.i.posthog.com`, `disable_geoip=False`, có tự bắt exception (`S:infrastructure/analytics.py:44-66`).
  - Event `exception` gửi `str(error)`, có thể chứa đường dẫn hoặc tiêu đề cửa sổ.

### 4.8 Screenshot
- Backend: dxcam/DXGI chỉ khi vùng nằm gọn trong một output, rồi mss, rồi `PIL.ImageGrab` (`S:desktop/screenshot.py:67, 198-413`).
- PNG, Lanczos thu nhỏ để vừa 1920×1080 (`S:tools/_snapshot_helpers.py:22-35`).
- Process là per-monitor DPI aware (`S:uia/core.py:1832-1852`). Response ghi rõ hệ số nhân và gốc tọa độ (`S:tools/_snapshot_helpers.py:52-68`).

### 4.9 Bảng claim cũ
| Claim cũ | Kết luận |
|---|---|
| MIT, Python 3.14+ | **CONFIRMED**. Version trong repo là 0.8.9 |
| Danh sách 21 tool | **CONFIRMED** (`S:tools/__init__.py:18-32`) |
| `WaitFor` chờ text, cửa sổ hoặc element | **CONFIRMED**, bằng poll |
| `Snapshot` có element id | **REFUTED một phần**: id chỉ vẽ trên ảnh; text chỉ có tâm `(x,y)` |
| Dùng thư viện Python-UIAutomation | **CONFIRMED**, dạng vendored và đã sửa |
| "Hành động bằng tọa độ và input giả lập" | **CONFIRMED** |
| XAML shell bỏ qua drag giả lập | Chỉ là mô tả trong tool (`S:tools/input.py:346-349`), không có code xử lý |
| Cài thành Scheduled Task theo user; không thấy màn khoá/UAC (trước là UNVERIFIED) | **CONFIRMED** (§4.6) |
| Telemetry bật mặc định | **CONFIRMED** |
| Không có verdict hay report | **CONFIRMED** |

---

## 5. UFO² (`microsoft/UFO`)

Đường dẫn tính từ gốc repo. Thư viện ghim: pywinauto 0.6.8, uiautomation 2.0.18, pyautogui 0.9.54 (`requirements.txt`). Mọi hành vi nằm bên trong pywinauto đều là **UNVERIFIED**, vì không đọc code thư viện đó.

### 5.1 Cây accessibility
- **Gần như giống hệt rcli:** `window_elem_com_ref.FindAllBuildCache(TreeScope_Descendants, condition, cacheRequest)` (`ufo/automator/ui_control/inspector.py:251-255`).
  - Cache chỉ có ControlType, Name, BoundingRectangle (`inspector.py:325-331`).
  - Condition là AND của `IsEnabled`, `IsOffscreen==False`, `IsControlElement`, và OR theo 15 control type (`inspector.py:334-370`).
  - Đọc tối đa **500** phần tử: `min(com_elem_array.Length, 500)` (`inspector.py:266`, đã kiểm tay).
- **Phạm vi:**
  - AppAgent chỉ lấy cửa sổ HostAgent đã chọn, có gọi `set_focus()` (`ufo/client/mcp/local_servers/ui_mcp_server.py:226, 782`).
  - HostAgent lấy danh sách cửa sổ desktop cùng ảnh toàn màn hình.
- **Tới LLM chỉ có `{id, name, type}`** dạng JSON (`ufo/agents/processors/strategies/app_agent_processing_strategy.py:853-856`). Không có rect, value hay state.
- **Hybrid UIA + vision:**
  - Tùy chọn, bật khi `CONTROL_BACKEND` chứa `"omniparser"`.
  - Giữ mọi control UIA, chỉ thêm box OmniParser có IoU ≤ 0.1 với mọi box UIA (`ufo/automator/ui_control/screenshot.py:1317-1345`; `config/ufo/system.yaml:12`).

### 5.2 Hành động semantic
- Tool của AppAgent: `click_input`, `click_on_coordinates`, `drag_on_coordinates`, `set_edit_text`, `keyboard_input`, `wheel_mouse_input`, `texts`, `wait`, `summary` (`ui_mcp_server.py:285-628`).
  - Mặc định `CLICK_API="click_input"` (chuột giả lập) và `INPUT_TEXT_API="type_keys"` (`ufo/automator/ui_control/controller.py:98-259`; `system.yaml:50-54`).
- **Không có tool select, toggle, expand hay invoke.** Code UFO không gọi UIA pattern nào để hành động. Chỉ đọc LegacyIAccessible ở `inspector.py:617,624`.
- **API action:** COM `win32com.client.Dispatch("Word.Application"/…)` cho Word, Excel, PowerPoint (`ufo/automator/app_apis/basic.py:42`). LLM tự chọn giữa API và GUI. `MCP_FALLBACK_TO_UI` được khai báo nhưng không có code nào đọc.

### 5.3 Xác minh hiệu ứng và EvaluationAgent
- **Mỗi bước:**
  - Kiểm enabled/visible trước khi làm (`ufo/automator/action_execution.py:105-110`) và khớp id với name (`ui_mcp_server.py:170-188`).
  - Read-back chỉ có khi `INPUT_TEXT_API=set_text` (`controller.py:210-215`).
  - Phần còn lại giao cho LLM: prompt yêu cầu "Compare the current screenshot with the screenshot of the last step" (`ufo/prompts/share/base/app_agent.yaml:76`).
- **EvaluationAgent:**
  - Một lần gọi LLM sau khi session xong (`ufo/module/basic.py:525-528, 845-876`; `ufo/agents/agent/evaluation_agent.py:113-137`).
  - Đầu vào là trajectory đọc lại từ `response.log` cùng ảnh (`ufo/prompter/eva_prompter.py:93-284`).
  - `EVA_ALL_SCREENSHOTS` **mặc định True**: lấy ảnh `selected_controls` của từng bước (đã annotate) cùng ảnh cuối, tối đa 40 ảnh. False thì chỉ lấy ảnh đầu và ảnh cuối.
  - Output `{reason, sub_scores[{name, evaluation}], complete: yes|no|unsure}` (`ufo/prompts/evaluation/evaluate.yaml:29-38`). Schema pydantic để `complete: str`, không ép enum.
  - Kết quả ghi vào `evaluation.log` (`basic.py:878-895`).

### 5.4 Cửa sổ và app
- Liệt kê: `Desktop(backend="win32").windows()` lọc visible, bỏ IME (`inspector.py:179-204`).
- Chọn cửa sổ: `set_focus()`, tùy chọn `maximize()`.
- Mở app: `run_shell` theo allowlist rồi `subprocess.Popen` và `sleep(5)` (`ufo/client/mcp/local_servers/cli_mcp_server.py:29-70, 137-162`).
- Không có code riêng cho menu hay dialog.

### 5.5 Chờ
- Không có wait-for. `wait_enabled`/`wait_visible` poll 0.5 s trong 10 s và chỉ cảnh báo khi hết giờ (`controller.py:395-419`).
- Tool `wait` của LLM là sleep ≤ 300 s. `run_shell` ngủ cố định 5 s.

### 5.6 Phiên và quyền
- Không có code về DPI, WTS, session 0, service, secure desktop, màn khoá hay UAC.
- Có xử lý **RDP bị ngắt**: lùi về `PrintWindow(PW_RENDERFULLCONTENT)`, coi ảnh BitBlt đen hoàn toàn là dấu hiệu RDP ngắt (`ufo/automator/ui_control/screenshot.py:107-120, 136-204, 214-284`).
- Kết luận "chỉ trong phiên user": **phù hợp với code**, dù code không nói rõ.

### 5.7 Artifact
| File | Code ghi |
|---|---|
| `logs/<task>/response.log` (JSONL, mỗi bước một dòng) | `ufo/agents/processors/core/processing_middleware.py:109-126` |
| `request.log` (JSONL, gồm ảnh base64) | `app_agent_processing_strategy.py:1212-1216` |
| `evaluation.log` | `ufo/module/basic.py:878-895` |
| `action_step{N}.png`, `_annotated`, `_selected_controls`, `_concat` | `app_agent_processing_strategy.py:127, 546-555, 888, 1441-1450` |
| `ui_trees/ui_tree_step{N}.json` (khi `SAVE_UI_TREE`, mặc định False) | `app_agent_processing_strategy.py:145-150` (đã kiểm tay); nội dung ở `ufo/automator/ui_control/ui_tree.py:48-99` |
| `output.md` (report markdown từ trajectory) | `basic.py:530-532`, `ufo/trajectory/parser.py:371-460` |

- Không có video.

### 5.8 Screenshot
- Cửa sổ: `capture_as_image()` → `PrintWindow(PW_RENDERFULLCONTENT)` → desktop (`screenshot.py:82-133`).
- Desktop: `ImageGrab.grab` → `BitBlt` → `PrintWindow`.
- PNG, **không resize** trên đường chính.
- Label được dán ở góc trên-trái control, tọa độ trừ theo left/top của cửa sổ (`screenshot.py:536-610, 798-919`).
- Click theo tọa độ nhận phân số 0..1 của rect cửa sổ (`controller.py:421-437`).

### 5.9 Bảng claim cũ
| Claim cũ | Kết luận |
|---|---|
| UFO² chỉ cho Windows | **CONFIRMED** cho desktop agent (`ui_mcp_server.py:17-24`) |
| Thấy "Windows UIA, Win32, WinCOM native control" | **REFUTED một phần**: WinCOM chỉ là API hành động và đọc dữ liệu cho Office, không dùng để quan sát control |
| "Hybrid UIA + vision" | **CONFIRMED** nhưng là tùy chọn, mặc định chỉ UIA |
| "GUI clicks + API calls" | **CONFIRMED**. Không có fallback API→GUI tự động |
| EvaluationAgent hoàn toàn dùng LLM, trên trajectory và ảnh | **CONFIRMED** |
| EvaluationAgent tách sub-goal | **REFUTED một phần**: chỉ là prompt yêu cầu LLM tự đặt `sub_scores`; code không tách |
| Kết quả yes, no, unsure | **CONFIRMED** trong prompt; schema không ép |
| `EVA_ALL_SCREENSHOTS=False` chỉ dùng ảnh đầu và cuối | **CONFIRMED**. Mặc định là True |
| `request.log`, `response.log`, `evaluation.log`, ảnh mỗi bước | **CONFIRMED** |
| `ui_tree/step_N.json` | **REFUTED đường dẫn**: thật là `ui_trees/ui_tree_step{N}.json`. Nội dung (`control_type`, `rectangle`) và cờ `SAVE_UI_TREE` **CONFIRMED** |
| MIT | **CONFIRMED** |

---

## 6. rcli (commit `65609dd`, v0.3.0)

Đường dẫn tính từ `crates/`.

### 6.1 Cây accessibility (Mark)
- **Port:** `Platform::accessibility(display) -> Option<Vec<UiElement>>`. `None` nghĩa là dùng OCR (`agent/src/platform.rs:18-20`). `UiElement` chỉ có `role, text, x, y, width, height` (`platform.rs:72-79`). Mark có thêm `number` và `display` (`protocol/proto/agent.proto:314-323`, `agent/src/marks.rs:26-30`).
- **Windows:**
  - `CoCreateInstance(CUIAutomation)` → `ElementFromHandle(GetForegroundWindow())`. Nếu cửa sổ foreground không nằm trên display đó thì lấy cửa sổ ở tâm display (`agent/src/windows_platform.rs:166-182`).
  - `FindAllBuildCache(TreeScope_Subtree, IsOffscreen=false, cache{ControlType, Name, BoundingRectangle})` (`:183-188`).
  - Map role cho 10 nhóm control type (`:633-649`).
  - **Không giới hạn số node và không đặt UIA timeout.**
- **macOS:**
  - `AXFocusedApplication` (`agent/src/macos.rs:200-201`), DFS qua `AXChildren`, tối đa 3000 node (`:194, 207-211`), `set_messaging_timeout(0.5)` (`:203`).
  - Text là attribute đầu tiên có giá trị trong `AXTitle/AXDescription/AXValue/AXPlaceholderValue`. Mỗi attribute là một lần `AXUIElementCopyAttributeValue` riêng (`:213-218`).
  - Cần `AXIsProcessTrusted` (`:197`).
- **Linux:**
  - Gốc là registry, tức **mọi app** (`agent/src/linux/atspi.rs:21`).
  - Lọc `SHOWING`+`VISIBLE`, tối đa 3000 node, timeout 500 ms mỗi method (`atspi.rs:11-15, 31-35, 55`).
  - Đọc `GetRoleName`, `Name`, `Component.GetExtents(0)` (`atspi.rs:60-82`).
  - Bật `org.a11y.Status.IsEnabled` (`atspi.rs:51-52`).
  - Cùng cách đi cho X11 và Wayland (`agent/src/linux/mod.rs:190-195`). Comment ở `platform.rs:19` vẫn nói Wayland trả `None`, không khớp với code.
- **Không có:** hierarchy, value riêng, state, id ổn định. Mark gắn với một Observation; Mark cũ trả `stale_mark` (`agent/src/executor.rs:214-219`).
- **Output:** bảng Mark trong structured JSON, cùng ảnh có vẽ khung và số (`marks.rs:74-108`, `cli/src/mcp.rs:119-132`).
- **OCR:**
  - Windows: `Windows.Media.Ocr` (`windows_platform.rs:205-240`);
  - macOS: `VNRecognizeTextRequest` (`macos.rs:426-443`);
  - Linux: CLI `tesseract` (`linux/mod.rs:197-214`).

### 6.2 Hành động semantic
- **Không có.** `Input` chỉ gồm `Move/Press/Scroll/Text/Key/KeyDown/KeyUp/Sas` (`platform.rs:83-98`). Mark hay text được đổi thành tâm của Mark (`marks.rs:52-54`).
- Bên dưới:
  - Windows: enigo + `SendInput` (`windows_platform.rs:126-164, 492-506`);
  - macOS: enigo (CGEvent) (`macos.rs:376-410`);
  - Linux X11: XTest qua enigo (`linux/x11.rs:58-75`);
  - Linux Wayland: `/dev/uinput` (`linux/uinput.rs:1-6`).

### 6.3 Xác minh hiệu ứng
- Không có. Thứ có sẵn là chờ màn hình đứng yên tối đa 1 s, poll 100 ms (`agent/src/observer.rs:17-20, 202-221`), và `unchanged: true` khi JPEG trùng byte với ảnh trước (`observer.rs:87-93`).

### 6.4 Cửa sổ và app
- Không có trong `Platform` (`platform.rs:11-61`). Chỉ có `processes()` và `power(Lock)`.

### 6.5 Chờ
- Action `wait_ms` (`agent.proto:272`), tổng ≤ 5000 ms mỗi batch (`server/src/api/actions.rs:47`), cùng settle ở trên. Không có `wait_for`.

### 6.6 Phiên và quyền
- **Kiến trúc:** service root/SYSTEM cộng một session helper trong phiên console. Helper lo displays, capture, input, accessibility, state và Lock. Shell, power và metrics ở lại service (`agent/src/session.rs:1-7, 83-221`). Wire protocol `Request {State, Displays, Capture, Input, Accessibility, Lock}` (`session.rs:38-54`).
- **Windows:**
  - Helper chạy **SYSTEM trong session console**: `DuplicateTokenEx` + `SetTokenInformation(TokenSessionId)`; lùi về `WTSQueryUserToken` (`agent/src/windows_session.rs:233-269, 334-345`).
  - Trước mỗi capture, input hay UIA, helper chuyển sang input desktop bằng `OpenInputDesktop` + `SetThreadDesktop` (`windows_platform.rs:547-578`).
  - Ctrl+Alt+Del qua `SendSAS(false)` (`:129-132`).
- **macOS:**
  - LaunchAgent `LimitLoadToSessionType = LoginWindow, Aqua` (`cli/src/macos_install.rs:90-98`).
  - Input ở màn đăng nhập bị từ chối (`session.rs:31, 167-174`).
  - Session state lấy từ CGSession (`macos.rs:447-454`).
- **Linux:**
  - Service tìm phiên seat0 bằng logind rồi chạy helper bằng `runuser -u <user> -- env DISPLAY/XAUTHORITY | WAYLAND_DISPLAY/XDG_RUNTIME_DIR/DBUS_SESSION_BUS_ADDRESS …` (`agent/src/linux/session.rs:282-311, 584-589`).
  - Trên X11, helper không được đặt `DBUS_SESSION_BUS_ADDRESS`. AT-SPI vẫn nối được vì zbus 5.19 lùi về `$XDG_RUNTIME_DIR/bus`, rồi `/run/user/<euid>/bus` (`~/.cargo/registry/src/*/zbus-5.19.0/src/address/mod.rs:67-81`).
- **Chạy được ở màn khoá, màn đăng nhập và UAC:** có code thật cho Windows. Linux greeter nhận XTest. macOS chỉ xem được màn đăng nhập. **Đây là điểm không công cụ nào trong năm công cụ có**: Cua, Windows-MCP và Peekaboo chỉ phát hiện màn khoá để báo lỗi (§1.6, §4.6, §3.6).

### 6.7 Artifact
- MCP trả ảnh inline và clip dạng blob `rcli://clip.mp4` (`cli/src/mcp.rs:119-132`). Không ghi ra đĩa.
- CLI `--out` ghi `display-<N>.jpg`, `display-<N>-<at_ms>ms.jpg`, `clip.mp4` (`cli/src/lib.rs:410-437`).
- Clip: H.264 OpenH264, 10 fps, tối đa 5 s (`agent/src/clip.rs:19-22`).
- Không có thư mục run, không có step log, không có trajectory.

### 6.8 Screenshot
- **Capture:**
  - Windows: xcap (GDI) sau khi chuyển input desktop (`windows_platform.rs:117-124`);
  - macOS: `CGDisplayCreateImage`, API deprecated; có `shortcut:` ghi chú (`macos.rs:353-374`). Peekaboo cố ý không dùng API này;
  - Linux X11: `GetImage` qua x11rb (`linux/x11.rs:50`);
  - Linux Wayland: portal ScreenCast + PipeWire, có restore token (`linux/screencast.rs:1-7, 150-175`).
- Thu nhỏ để cạnh dài là 1280, không phóng to (`agent/src/coords.rs:6-16`). JPEG chất lượng 70 (`observer.rs:21`).
- Đổi tọa độ: `to_real`/`to_image` theo tỉ lệ ảnh/display (`coords.rs:18-33`). Windows map ra virtual screen 0..65535 có `MOUSEEVENTF_VIRTUALDESK` (`windows_platform.rs:492-512`).

### 6.9 Câu hỏi mở 1: `exec`/`shell` chạy ở đâu
Ở cả ba OS, `SessionPlatform::shell` và `pty` gọi thẳng `inner` trong service (`session.rs:194-199`; Linux `linux/session.rs:547-552`). Helper không bao giờ chạy lệnh.

| OS | User | Session / bootstrap | Môi trường |
|---|---|---|---|
| Windows | **Luôn là SYSTEM**: `shell()` chạy `powershell.exe` như chính process service và bỏ qua `_run_as` (`windows_platform.rs:347-379`). Service là LocalSystem (`docs/usage.md:67`) | Session 0, window station của service (`windows_session.rs:2`) | Env của SYSTEM, `-NoProfile -NonInteractive`, `CREATE_NO_WINDOW` (`windows_platform.rs:350-356`) |
| macOS | Terminal User qua `sudo -n -u <user> -i /bin/zsh -c …`; root thì chạy thẳng (`macos.rs:278-298`) | Process con của LaunchDaemon, tức bootstrap hệ thống. Code không dùng `launchctl asuser` | Môi trường login của user do `sudo -i` tạo. Không có app GUI nào chịu trách nhiệm TCC. Tool GUI có chụp được ảnh hay đọc được AX ở đây không: **UNVERIFIED** (Cua phải kiểm `sessionHasGraphicAccess` vì lý do này, `platform-macos/src/session.rs:1-45`) |
| Linux | Terminal User qua `sudo -n -H -u <user> -- sh -c …`; chạy thẳng nếu đã là user đó (`linux/shell.rs:25-41, 50-53`) | Process con của systemd unit root | Code **không đặt** `DISPLAY`, `XAUTHORITY`, `WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS` hay `XDG_RUNTIME_DIR`. Phần env còn lại phụ thuộc chính sách `env_reset` của sudo trên từng máy: **UNVERIFIED** |

Hệ quả cho các tool ngoài chạy qua `rcli exec`:
- **Linux:**
  - Cua tự tìm session bus (`session_bus.rs:41-130`) và `XAUTHORITY`, nhưng không tìm `DISPLAY`.
  - agent-desktop không tự tìm gì.
  - Cả hai đều cần `cua-driver serve` hoặc phiên đúng chạy sẵn. CLI của Cua cần daemon có socket tại `~/.cache/…` (`daemon.rs:149-168`); `sudo -H` đặt đúng `HOME`.
- **Windows:** mọi tool đều ở session 0. Pipe `\\.\pipe\cua-driver` có nhận client SYSTEM hay không: **UNVERIFIED**.
- **macOS:** Bridge socket của Peekaboo nằm theo `HOME` của user. Sau `sudo -u -i` thì `HOME` là của user, nhưng host Bridge kiểm Team ID của peer nên lệnh có thể bị từ chối: **UNVERIFIED**.

### 6.10 Câu hỏi mở 2: `_run_as` bị bỏ qua trên Windows là bug
- **ADR 0004 không giải thích việc này.** Dòng 8 viết "`--as root` (the only route on Windows) follows the Device's root setting". Câu này ngụ ý: không có `--as root` thì chạy bằng Terminal User. Còn `Account::User` được tạo đúng theo Device Settings (`agent/src/lib.rs:814`).
- **Lịch sử:**
  - Ticket 21 ghi đây là shortcut, vì khi đó Agent chạy trong phiên user (`.scratch/easy-setup/issues/21-windows-adapter-logged-in.md:32`).
  - Ticket 22 ghi: "`shell`/`pty` still run as the account the Agent runs as (SYSTEM in the service), not as the Terminal User; the ticket-21 shortcut stands until ticket 26" (`.scratch/easy-setup/issues/22-windows-service-installer.md:91-92`).
  - **Không có ticket 26 nào làm việc này.** `easy-setup` chỉ có ticket 01–24. `.scratch/remote-cli/issues/26-p2p-media-cli-mcp.md` là P2P media, đã resolved, không liên quan.
  - Comment `shortcut:` trong code (`windows_platform.rs:347-348`) vẫn nói "ticket 22's SYSTEM service starts it as the Terminal User or SYSTEM", tức đã lỗi thời.
- **Hậu quả (đọc từ code):**
  1. Device Settings `root never` **không có tác dụng trên Windows**: mọi `exec`/`shell` đều chạy SYSTEM.
  2. `ExecResult.as_root` được tính từ `account` (`lib.rs:911`), nên báo `false` dù lệnh chạy SYSTEM. Comment proto ghi rõ field này là "who it actually ran as" (`agent.proto:387`).
  3. Audit cũng ghi theo `as_root` này nên sai theo.
  4. Testkit chạy trên FakePlatform nên không bắt được lỗi này (`testkit/tests/terminal_user.rs`).
- **Kết luận:** đây là **bug**, gồm một shortcut đã ghi nhận nhưng ticket theo dõi không còn tồn tại, cộng một field báo sai. Phần sửa có sẵn nửa đường: xem 6.11.

### 6.11 Câu hỏi mở 3: có session helper để `exec` dùng lại không
Có, ở cả ba OS. Nhưng wire protocol hiện chưa có lệnh chạy shell (`session.rs:38-54`).
- **Windows:**
  - Helper chạy SYSTEM trong session console, trên `WinSta0` (`windows_session.rs:233-269`). Chạy lệnh ở đó thì được session GUI nhưng sai danh tính user.
  - Hàm `spawn_as_console_user` (`windows_session.rs:289-332`) đã làm đúng phần khó: `WTSQueryUserToken` + `CreateEnvironmentBlock` + `lpDesktop="winsta0\default"` + `CreateProcessAsUserW`. Hiện nó chỉ dùng để mở GUI (`cli/src/gui.rs:666`). Nó thiếu pipe stdout/stderr, thiếu chờ exit code, và chỉ áp được cho user đang ở console, chưa cho một Terminal User bất kỳ.
- **macOS:** helper LaunchAgent trong phiên Aqua chạy bằng user console. Service tin uid 0 hoặc console uid (`agent/src/macos_session.rs:22-27`). Helper mang quyền TCC đã cấp cho rcli (`macos.rs:690-700`). Chạy lệnh trong helper sẽ có đúng bootstrap GUI. Tool con có được tính TCC theo rcli hay không: **UNVERIFIED**.
- **Linux:** `session_env(target)` (`linux/session.rs:282-297`) đã sinh đúng các biến của phiên, gồm cả session bus trên Wayland. `exec` chỉ cần thêm các biến này khi Terminal User trùng user của phiên console.

---

## 7. Bảng năng lực × công cụ

Ký hiệu: ✅ có, ⚠️ có một phần, ❌ không có. Tiền tố: `cua:` = `libs/cua-driver/rust/crates/`; `ad:` = `crates/` của agent-desktop; `pb:` = PB: ở §3; `wm:` = `src/windows_mcp/`; `ufo:` = gốc repo UFO; `rcli:` = `crates/` của repo này.

| Năng lực | rcli | Cua Driver | agent-desktop | Peekaboo | Windows-MCP | UFO² |
|---|---|---|---|---|---|---|
| Cây có hierarchy | ❌ Mark phẳng (`rcli:agent/src/platform.rs:72-79`) | ⚠️ `depth`, `parent_index` (`cua:platform-windows/src/tools/get_window_state.rs:60-115`) | ✅ cây lồng (`ad:core/src/tree/node.rs:8-31`) | ❌ phẳng (`pb:Services/UI/ElementDetectionService.swift:256`) | ⚠️ cây ASCII theo container (`wm:tree/views.py:150-178`) | ❌ list `{id,name,type}`; cây đầy đủ chỉ ở log (`ufo:ufo/automator/ui_control/ui_tree.py:48-99`) |
| Value tới agent | ⚠️ chỉ khi là text đầu tiên (macOS) (`rcli:agent/src/macos.rs:213-216`) | ✅ (`cua:platform-macos/src/tools/get_window_state.rs:1187-1290`) | ✅ (`ad:core/src/tree/node.rs`) | ✅ (`…/UIElementSummary.swift:9-27`) | ✅ LegacyIAccessible Value (`wm:tree/service.py:655-797`) | ❌ (`ufo:…/app_agent_processing_strategy.py:853-856`) |
| State (checked/enabled/focused/selected/expanded) | ❌ | ⚠️ enabled, selected (`cua:platform-linux/src/atspi/native.rs:1744-1790`) | ✅ đủ cả năm (`ad:windows/src/tree/states.rs:70-181`) | ⚠️ enabled, selected, focused (`pb:…/DetachedAXObservationWorker.swift:109-125`) | ✅ focused, toggle, selected, expand (`wm:tree/service.py:655-797`) | ❌ |
| Phạm vi | Win: cửa sổ foreground; mac: app focus; Linux: mọi app | 1 app/cửa sổ | cửa sổ focus hoặc chọn | 1 cửa sổ | foreground + popup/taskbar (`wm:desktop/service.py:230`) | cửa sổ đã chọn |
| Giới hạn node/thời gian | ⚠️ mac/Linux 3000 node; Win không có (`rcli:agent/src/windows_platform.rs:183-188`) | ✅ (`cua:platform-windows/src/uia/mod.rs:44-46`) | ✅ (`ad:core/src/locator/live_locator/observation_budget.rs:3-5`) | ✅ (`pb:…/DesktopObservationRequestModels.swift:35-41`) | ✅ 500 (`wm:tree/budget.py:19`) | ✅ 500 (`ufo:ufo/automator/ui_control/inspector.py:266`) |
| Action semantic | ❌ (`rcli:agent/src/platform.rs:83-98`) | ✅ UIA/AX/AT-SPI (`cua:platform-windows/src/tools/click.rs:900-955`) | ✅ (`ad:windows/src/actions/dispatch.rs:192-260`) | ✅ AX (`pb:Services/UI/ActionInputDriver.swift:175-224`) | ❌ (`wm:desktop/service.py:1060-1071`) | ❌ (`ufo:…/controller.py:98-111`) |
| Xác minh hiệu ứng | ❌ chỉ settle/`unchanged` (`rcli:agent/src/observer.rs:87`) | ✅ `effect` + evidence (`cua:cua-driver-core/src/action_record.rs:583-587`) | ✅ `delivery` (`ad:core/src/actions/delivery_semantics.rs:47-66`) | ✅ read-back (`pb:…/ActionInputDriver.swift:816-880`) | ⚠️ read-back khi Type (`wm:desktop/service.py:1078-1114`) | ⚠️ do LLM tự so ảnh (`ufo:ufo/prompts/share/base/app_agent.yaml:76`) |
| Liệt kê, focus cửa sổ | ❌ | ✅ (`cua:platform-windows/src/input/inject.rs:119-138`) | ✅ (`ad:windows/src/system/window/window_activate.rs:228-270`) | ✅ (`pb:Utilities/FocusUtilities.swift:1206-1223`) | ✅ (`wm:desktop/service.py:628-732`) | ⚠️ set_focus (`ufo:…/ui_mcp_server.py:226`) |
| Mở app | ❌ (chỉ qua `exec`) | ✅ (`cua:platform-linux/src/tools/launch_app.rs:19-48`) | ✅ (`ad:windows/src/system/launch.rs:221`) | ✅ (`pb:Services/System/ApplicationService.swift:215-281`) | ✅ (`wm:desktop/service.py:590-626`) | ⚠️ allowlist (`ufo:…/cli_mcp_server.py:29-70`) |
| `wait_for` có điều kiện | ❌ chỉ `wait_ms` (`rcli:protocol/proto/agent.proto:272`) | ✅ `verify_state` (`cua:cua-driver-core/src/expectation.rs:25-29`) | ✅ (`ad:core/src/commands/wait_mode.rs:7-37`) | ✅ `verify` (`…/VerifyCommand.swift:3-14`) | ✅ `WaitFor` (`wm:tools/input.py:466-511`) | ❌ (`ufo:…/controller.py:395-419`) |
| Artifact mỗi bước | ⚠️ CLI `--out` (`rcli:cli/src/lib.rs:410-437`) | ✅ `turn-NNNNN/` + mp4 (`cua:cua-driver-core/src/recording.rs:1-12`) | ✅ pre/post PNG khi `--artifacts` (`ad:core/src/trace/trace_artifacts.rs:47-146`) | ⚠️ snapshot, video watch (`pb:…/SnapshotManager+Helpers.swift:122-133`) | ❌ | ✅ log + ảnh + `output.md` (`ufo:ufo/module/basic.py:530-612`) |
| Video | ⚠️ clip ≤ 5 s (`rcli:agent/src/clip.rs:19-22`) | ✅ `recording.mp4` | ❌ | ✅ `capture.mp4` (`pb:Services/Capture/VideoWriter.swift:72-73`) | ❌ | ❌ |
| Màn khoá / đăng nhập / UAC | ✅ điều khiển được (`rcli:agent/src/windows_platform.rs:547-578`) | ⚠️ chỉ phát hiện (`cua:platform-windows/src/diagnostics.rs:155-260`) | ❌ | ⚠️ chỉ phát hiện (`pb:Services/System/DesktopSessionLockState.swift:4-13`) | ⚠️ phát hiện rồi tắt (`wm:desktop/control_win32.py:221-252`) | ❌ |
| Chạy từ service / root | ✅ service + helper (`rcli:agent/src/session.rs:83-221`) | ⚠️ cảnh báo; Linux tự tìm bus (`cua:platform-linux/src/session_bus.rs:41-130`) | ❌ | ❌ | ❌ | ❌ |
| Đổi tọa độ ảnh → màn hình | ✅ (`rcli:agent/src/coords.rs:18-33`) | ✅ (`cua:cua-driver-core/src/element_frame.rs:39-50`) | ⚠️ không resize (`ad:core/src/commands/screenshot.rs:28-41`) | ✅ (`pb:Services/Core/CaptureCoordinateMapper.swift:37-79`) | ✅ (`wm:tools/_snapshot_helpers.py:52-68`) | ⚠️ theo rect cửa sổ (`ufo:…/controller.py:421-437`) |

---

## 8. Nâng cấp cho rcli

Chỉ gồm những khoảng trống đã xác nhận trong code rcli (§6). Thứ tự theo giá trị cho E2E do agent điều khiển: trước tiên là assert tất định, rồi chờ đúng lúc, rồi hành động đáng tin, rồi bằng chứng. Không có code nào được viết ở đây.

**License chung:** rcli là AGPL-3.0. Code MIT (Cua, Peekaboo, AXorcist, Windows-MCP, UFO) và Apache-2.0 (agent-desktop, phần `uiautomation` vendored trong Windows-MCP) **đều được đưa vào AGPL-3.0**:
- MIT: giữ copyright và permission notice.
- Apache-2.0: giữ LICENSE và NOTICE, ghi rõ file đã sửa. Apache-2.0 tương thích một chiều với GPLv3/AGPLv3.
- Weights detector OmniParser (AGPL-3.0) cũng tương thích, nhưng chỉ cần nếu dùng perception.
- **Không có gì không tương thích.**
- Rủi ro không thuộc về license: Peekaboo, agent-desktop và Cua (macOS) dùng API private (`SLEventPostToPid`, `_AXUIElementGetWindow`, `_AXUIElementRemoteTokenCreate`). Các API này có thể gãy theo bản macOS.

### 1. Mark có state, value và cha (assert tất định). Kích thước: M
- **Tham khảo tốt nhất:** agent-desktop cho danh sách property và state; Cua cho cách suy action từ pattern có trong cache.
  - **Windows:** giữ `FindAllBuildCache` nhưng thêm vào CacheRequest:
    - property: `IsEnabled`, `HasKeyboardFocus`, `IsPassword`, `Value.Value`, `Toggle.ToggleState`, `ExpandCollapse.ExpandCollapseState`, `SelectionItem.IsSelected`, `RangeValue.Value`, `LegacyIAccessible.State/Value/DefaultAction`, các `Is*PatternAvailable`, `RuntimeId` (`ad:windows/src/tree/properties/property_ids.rs:135-178`);
    - pattern: dùng `AddPattern` cho LegacyIAccessible (cách của Windows-MCP, `wm:tree/cache_utils.py:57-125`).

    Cách này không thêm round-trip. Cha-con: đổi sang `BuildUpdatedCache(TreeScope_Subtree)` + `GetCachedChildren` như Cua (`cua:platform-windows/src/uia/mod.rs:397-433`) để biết `parent_index`.
  - **macOS:** đổi các `AXUIElementCopyAttributeValue` lẻ (`rcli:agent/src/macos.rs:212-218`) thành một `AXUIElementCopyMultipleAttributeValues` mỗi node, thêm AXEnabled, AXFocused, AXSelected, AXExpanded, AXIdentifier, AXValue (`pb:…/DetachedAXObservationWorker.swift:109-125`; `ad:macos/src/tree/properties/node_attribute_names.rs:17-42`).
  - **Linux:** `GetState` đã được gọi (`rcli:agent/src/linux/atspi.rs:31`). Chỉ cần giữ thêm các bit CHECKED, ENABLED, FOCUSED, SELECTED, EXPANDED, và đọc `Value.CurrentValue`/`Text`.
- **Chỗ cắm:**
  - thêm field vào `UiElement` (`rcli:agent/src/platform.rs:72-79`) và `Mark` (`rcli:protocol/proto/agent.proto:314-323`): `value`, `states` (repeated string hoặc bitmask), `parent`;
  - `marks::to_mark` (`rcli:agent/src/marks.rs:26-30`);
  - wire JSON của helper đã serialize `UiElement` bằng serde (`rcli:agent/src/session.rs:256`), nên field mới đi qua tự động.
- **Kèm theo (S):**
  - đặt `IUIAutomation2` ConnectionTimeout/TransactionTimeout (2 s/5 s như Windows-MCP, `wm:uia/core.py:114-121`);
  - probe `SendMessageTimeoutW(WM_NULL, SMTO_ABORTIFHUNG)` (`ad:windows/src/tree/automation.rs:235-289`);
  - ngân sách node cho Windows (rcli chưa có);
  - mở rộng phạm vi Windows sang popup/context menu/taskbar, ưu tiên dialog `IsModal` (`wm:desktop/service.py:230`, `wm:tree/service.py:917-927`);
  - macOS walk `AXFocusedWindow` cộng `AXWindows` thay vì cả app.

### 2. `wait_for` / `verify` trên Agent. Kích thước: M (cần mục 1 để có điều kiện về value/state)
- **Tham khảo tốt nhất:** `verify_state` của Cua và `verify` của Peekaboo, cùng một thiết kế: poll 100 ms, cần 2 mẫu ổn định liên tiếp, timeout mặc định 5 s và tối đa 10 s, vị từ `exists | value_equals | enabled | selected` trên selector `{role, label_contains}` (`cua:cua-driver-core/src/expectation.rs:25-29`; `pb:…/VerifyCommand.swift:3-14`).
  - agent-desktop có thêm `--text`, `--window`, `--event` (`ad:core/src/commands/wait_mode.rs:7-37`).
  - Windows-MCP chờ `text_exists` trên cây (`wm:tools/input.py:16-22`).
- **Riêng rcli:** có OCR fallback, nên `text` vẫn chờ được ở màn khoá và màn đăng nhập. Công cụ khác không làm được việc này.
- **Chỗ cắm:**
  - thêm `WaitFor { selector, predicate, timeout_ms }` vào `oneof kind` của `Action` (`rcli:protocol/proto/agent.proto:260-274`);
  - xử lý trong `Executor::step` (`rcli:agent/src/executor.rs:78-101`) bằng vòng `marks::detect` + so khớp, giống `by_text` (`rcli:agent/src/marks.rs:33-49`);
  - nâng hoặc tách `MAX_TOTAL_WAIT_MS` (`rcli:server/src/api/actions.rs:47`) và timeout request 10 s của Server (`rcli:agent/src/clip.rs:21` có nhắc);
  - thêm schema MCP vào `ACTION` (`rcli:cli/src/mcp.rs:44-48`);
  - trả lỗi có code, ví dụ `wait_timeout`.

### 3. Action semantic có kết quả `effect`. Kích thước: L (cần mục 1 cho ref)
- **Tham khảo tốt nhất:**
  - **Chuỗi action:** agent-desktop.
    - Windows `Invoke` → `LegacyIAccessible.DoDefaultAction`; `Toggle`; `ExpandCollapse`; `Value.SetValue` → `RangeValue.SetValue`; `SelectionItem.Select`; `ScrollItem.ScrollIntoView` (`ad:windows/src/actions/dispatch.rs:192-260`).
    - macOS `AXPress` → `AXOpen` → `AXConfirm`; ghi `AXValue`; ghi `AXSelectedText` để gõ (`ad:macos/src/actions/chain/chain_defs.rs:12-35`).
    - Linux `Action.DoAction` theo tên, `EditableText.SetTextContents` (`ad:linux/src/actions/action_call.rs:9-38`).
  - **Chống chiếm foreground:** Cua bọc mỗi lần gọi trong `EnableWindow(host, FALSE)` (`cua:platform-windows/src/uia/fg_bypass.rs:1-30`).
  - **Mô hình kết quả:** Cua `effect ∈ {confirmed, partial, unverifiable, suspected_noop, refused}`. Chỉ `confirmed` khi có read-back `value_readback`/`accessibility_readback`/`window_change`, thiếu thì hạ thành `unverifiable` (`cua:cua-driver-core/src/action_record.rs:583-587, 1030-1043`). Quy tắc read-back: `after == expected && after != before` (`cua:platform-windows/src/tools/type_text.rs:965-1027`), poll 20 ms trong 250 ms (`pb:…/ActionInputDriver.swift:816-880`).
  - **Báo lỗi trung thực:** `retry: safe` chỉ khi chưa giao được action (`ad:core/src/actions/delivery_semantics.rs:47-55`).
- **Chỗ cắm:**
  - method mới trong `Platform`, ví dụ `perform(display, element_ref, action) -> Effect` (`rcli:agent/src/platform.rs:11-61`);
  - op mới `Perform` trong `session::Request` (`rcli:agent/src/session.rs:38-54`), vì phải chạy trong helper;
  - field `effect` trong `ActResult` (`rcli:protocol/proto/agent.proto:288-299`);
  - action MCP mới, ví dụ `{"type":"press|set_value|toggle|select", mark|text, value?}` trong `ACTION` (`rcli:cli/src/mcp.rs:44-48`).
- **Ref:** Mark hiện chỉ sống trong một Observation. Cần định danh lại element lúc thực thi: đường dẫn chỉ số con + role + pid/thời điểm tạo process (`ad:windows/src/tree/resolve/resolve_descent.rs:115-131`), hoặc chấm điểm như Peekaboo (`pb:Services/UI/AutomationElementResolver.swift:505-575`). Không nên dùng token private của macOS.
- **Chưa kiểm:** helper Windows là SYSTEM. Gọi UIA pattern từ SYSTEM vào app của user có chạy đúng không: **UNVERIFIED**.

### 4. Ghi run: artifact mỗi bước và trajectory. Kích thước: S–M
- **Tham khảo tốt nhất:**
  - Cua: `turn-NNNNN/{action.json, before.png, after.png, before_state.json, after_state.json, evidence.json}` cùng `session.json` và `recording.mp4` (`cua:cua-driver-core/src/recording.rs:1-12, 743-860`).
  - UFO: `response.log` JSONL mỗi bước cộng `output.md` sinh từ JSONL (`ufo:ufo/agents/processors/core/processing_middleware.py:109-126`, `ufo:ufo/trajectory/parser.py:371-460`).
- **Chỗ cắm:** phía Controller, không đụng Agent.
  - `rcli mcp` có thể nhận một thư mục run (cờ hoặc env), mỗi lần `act`/`batch`/`observe` ghi `step-NNN/{request.json, result.json, display-N.jpg, marks.json}`, dùng lại `save_observation` (`rcli:cli/src/lib.rs:410-437`) trong `Mcp::call` (`rcli:cli/src/mcp.rs:134`).
  - Video dài: ghép frames của các bước, hoặc lặp clip; clip hiện giới hạn 5 s (`rcli:agent/src/clip.rs:21-22`).
- **Lưu ý chính sách:** docs nói audit trên Server không bao giờ lưu ảnh. Ghi ở Controller là chuyện khác và nên là tùy chọn bật tay.

### 5. `exec` trong phiên GUI của Terminal User, và sửa bug Windows. Kích thước: M
- **Lý do:** sửa bug ở §6.10. Đồng thời mở đường cho các tool ngoài (agent-desktop, Cua, Peekaboo) chạy qua `rcli exec` khi cần những thứ rcli chưa có.
- **Tham khảo:** không công cụ nào trong năm công cụ làm việc này. Cua chỉ tự tìm session bus (`cua:platform-linux/src/session_bus.rs:41-130`). Tham khảo chính là chính rcli:
  - Windows `spawn_as_console_user` (`rcli:agent/src/windows_session.rs:289-332`): thêm `STARTUPINFO.hStdOutput/hStdError` qua pipe kế thừa được, `WaitForSingleObject` và `GetExitCodeProcess`, kill cây process khi hết giờ.
    - Với Terminal User không phải user đang ở console thì `WTSQueryUserToken` không áp được. Khi đó cần `LogonUser` (cần mật khẩu, trái ADR 0005) hoặc S4U: **UNVERIFIED**, cần quyết định thiết kế.
  - Linux: thêm `session_env(target)` (`rcli:agent/src/linux/session.rs:282-297`) vào `sudo … env …` trong `linux/shell.rs:25-41` khi Terminal User trùng user của phiên.
  - macOS: chạy qua helper Aqua bằng op mới `Shell` trong `session::Request` (`rcli:agent/src/session.rs:38-54`), hoặc `launchctl asuser <uid>` trong `shell_argv` (`rcli:agent/src/macos.rs:278-298`).
- **Chỗ cắm:** `Platform::shell`/`pty`. Có thể thêm `Account::SessionUser`, hoặc giữ `Account::User` và để adapter tự chọn. Sửa luôn `ExecResult.as_root` cho đúng (`rcli:agent/src/lib.rs:911`) và cập nhật comment `shortcut:` ở `windows_platform.rs:347-348`.

### 6. Liệt kê, focus cửa sổ và mở app. Kích thước: M
- **Tham khảo tốt nhất:**
  - Windows: `EnumWindows` + `DWMWA_CLOAKED` + bỏ `WS_EX_TOOLWINDOW` (`ad:windows/src/system/window/window_enum.rs:61-94`). Focus: `AllowSetForegroundWindow` + `AttachThreadInput` + `SetForegroundWindow` + `SetWindowPos(HWND_TOP)` rồi poll `GetForegroundWindow` để xác nhận (`wm:desktop/service.py:628-732`).
  - macOS: `CGWindowListCopyWindowInfo` + `NSRunningApplication.activate` + `AXRaise`, xác nhận bằng cả AX lẫn NSWorkspace (`pb:Utilities/FocusUtilities.swift:1196-1236`).
  - Linux X11: `_NET_CLIENT_LIST` + `_NET_ACTIVE_WINDOW` có server timestamp (`cua:platform-linux/src/input/foreground.rs:409-545`).
  - Mở app: `Get-StartApps`/`shell:AppsFolder` (`wm:desktop/service.py:398-468`), `openApplicationAtURL:` (macOS), `.desktop Exec` (Linux).
- **Chỗ cắm:** method `windows()`/`focus_window()`/`launch()` trong `Platform`, op mới trong `session::Request` (phải chạy trong helper). Một tool MCP `windows` hoặc action `focus`. Mở app trên Windows phải chạy bằng token user (xem mục 5), không chạy bằng SYSTEM.

### 7. Capture macOS bằng ScreenCaptureKit. Kích thước: S–M, giá trị thấp hơn cho E2E
- `CGDisplayCreateImage` đã deprecated (`rcli:agent/src/macos.rs:353-354`). Peekaboo cố ý không dùng nó (`pb:Services/Capture/LegacyScreenCaptureOperator+ScreenArea.swift:43-46`). Cua dùng `SCScreenshotManager::capture_image` (`cua:platform-macos/src/capture.rs:550-606`).
- **Chỗ cắm:** `MacPlatform::capture`.
- Chạy được ở LoginWindow session hay không: **UNVERIFIED**, cần probe như ADR 0001.

### Không nên làm
- Tự chấm bằng LLM kiểu EvaluationAgent của UFO trong rcli. Việc này hợp với runner phía Controller hơn: đưa trajectory (mục 4) cho model chấm, trả `{reason, sub_scores, complete}` (`ufo:ufo/prompts/evaluation/evaluate.yaml:29-38`).
- Input nền bằng API private SkyLight, các hook takeover `WH_*_LL`, telemetry.

---

## 9. Đính chính cho hai file research trước

Không sửa hai file đó. Danh sách dưới đây là những chỗ code bác bỏ, toàn bộ hoặc một phần.

### `ai-device-level-e2e-testing.md`
1. **§0c, dòng 47:** "`exec` một lần theo Terminal User hoặc root/SYSTEM, theo Device Settings (ADR 0004)". **Sai trên Windows**: luôn chạy SYSTEM (`crates/agent/src/windows_platform.rs:347-379`), xem §6.10.
2. **§0f mục 2, dòng 62, và §2e dòng 138:** "Cua Driver trả `effect: …`" và "Only accessibility read-back sets `verified` to `true`". `verified` không có trong kết quả công khai. `confirmed` còn nhận `value_readback`, `window_change`, `browser_readback` (§1.3, §1.9).
3. **§2e, dòng 137:** bốn nấc cố định. Không có thang cố định. Với Chromium trên Windows, pixel đi trước UIA (§1.9).
4. **§2e, dòng 136:** "Extension này AGPL-3.0". Code crate là MIT. Chỉ weights detector là AGPL (§1.9).
5. **§2e, dòng 140, và bảng dòng 69:** "Không có recording hay trajectory dành cho agent". Có `start_recording`/`replay_trajectory` (§1.7).
6. **§2a, dòng 111:** UFO² "thấy … WinCOM native control". WinCOM chỉ dùng để hành động và đọc dữ liệu Office (§5.9).
7. **§2a, dòng 112:** EvaluationAgent "tách sub-goal". Chỉ là prompt yêu cầu LLM tự đặt `sub_scores`. Ngoài ra `EVA_ALL_SCREENSHOTS` mặc định là True (§5.3).
8. **§2a, dòng 113:** "`ui_tree/step_N.json`". Đúng là `ui_trees/ui_tree_step{N}.json` (§5.7).
9. **§2b, dòng 118:** Windows-MCP `Snapshot` "có element id". Id chỉ vẽ trên ảnh; text chỉ có tâm `(x,y)` (§4.1).
10. **§2c, dòng 125:** Peekaboo "macOS 15+". Chỉ package CLI là `.v15`; Core là `.v14` (§3.9).

### `open-source-computer-use.md`
1. **§4b, dòng 134:** "Every tool also runs from the shell as `cua-driver <tool> '<json-args>'`". Đúng cú pháp, nhưng cả `<tool>` lẫn `call` **đều cần daemon đang chạy** (`libs/cua-driver/rust/crates/cua-driver/src/cli.rs:2625-2631`). Bước "nhỏ nhất" ở dòng 272 (`rcli exec <dev> -- cua-driver list_apps '{}'`) sẽ thất bại nếu chưa có `cua-driver serve`.
2. **§4b, dòng 135, và bảng dòng 171:** "JSON có `effect`, `verified` và `path`". Structured result công khai chỉ có `effect`, `route`, `delivery`, `evidence`, `escalation`; `verified` và `path` bị bỏ (§1.3).
3. **§4b, dòng 133:** extension perception "AGPL-3.0-only". Xem mục 4 ở trên.
4. **§4b, dòng 140:** macOS "Directly spawning a raw `cua-driver serve` outside `CuaDriver.app` … is unsupported". Code chỉ cảnh báo, không chặn (`libs/cua-driver/rust/crates/cua-driver/src/responsibility.rs:176-200`).
5. **§4a, dòng 123, và bảng dòng 170:** agent-desktop "macOS 13+, Windows 10 1809+, Ubuntu 24.04+". Không có kiểm tra lúc chạy. Build macOS mặc định target 10.15. Wayland chỉ có trên GNOME (§2.9).
6. **§1a, dòng 40, và bảng dòng 172:** Peekaboo "Chỉ macOS 15+". Xem mục 10 ở trên.
7. **§1b, dòng 49:** `axorc` "Lọc được theo `AXValue`, `AXEnabled`, `AXFocused`". `find` chỉ có `--value`; hai attribute còn lại chỉ có qua JSON `raw` (§3.9).
8. **§1a, dòng 45, và "Chạy trên rcli" §3:** Bridge "dành cho SSH, LaunchAgent, Codex". Đây chỉ là docs. Code không nhận biết SSH hay launchd. Socket tính theo `HOME` của người gọi, host kiểm Team ID (§3.6). Câu "Đây là CLI macOS duy nhất … có tài liệu riêng" vẫn đúng về docs.
9. Các mục trước đây ghi **UNVERIFIED** nay đã có kết luận từ code:
   - Định dạng `screenshot` của agent-desktop: file hoặc base64.
   - Windows-MCP không thấy màn khoá và UAC: CONFIRMED.
   - Tên task `cua-driver-serve`: CONFIRMED (`libs/cua-driver/rust/crates/cua-driver/src/bundle.rs:70-76`).

---

## 10. Phần còn UNVERIFIED

- Gọi UIA pattern (Invoke/Toggle/Value) từ helper SYSTEM của rcli vào app của user có chạy đúng không. Ảnh hưởng tới nâng cấp 3 trên Windows.
- SYSTEM ở session 0 có mở được pipe `\\.\pipe\cua-driver` của daemon user không (DACL). Root trên macOS có vào được Bridge socket 0600 của Peekaboo không, và TCC nào áp dụng.
- Tool GUI chạy dưới `sudo -u <user> -i` từ LaunchDaemon (bootstrap hệ thống) có chụp được ảnh hay đọc được AX không.
- Chính sách `env_reset` của sudo trên Device Linux, tức những biến nào còn lại khi `exec`.
- `ScreenCaptureKit` có chạy trong LoginWindow session của macOS không.
- Phần text trong ToolResult của Cua có còn `path`/`verified` không (mới đọc phần structured).
- Mọi hành vi nằm bên trong thư viện pywinauto (UFO) và mặc định logon type của Task Scheduler (Windows-MCP).
- Số dòng trong các repo ngoài lấy từ báo cáo của agent con. Chỉ các claim chính nêu ở phần "Nguồn và cách làm" được mở lại kiểm tay.
