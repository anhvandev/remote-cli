# Nâng cấp cho rcli: tổng hợp từ README và mã nguồn của các công cụ computer-use / E2E (2026-11)

Ngày tổng hợp: 2026-11-13. rcli tại commit `65609dd` (v0.3.0).
File này **chỉ gộp lại**, không có tra cứu mới. Nguồn là 4 file:
- `computer-use-source-verification.md`, viết tắt **[SRC]**: đọc mã nguồn của Cua Driver, agent-desktop, Peekaboo/AXorcist, Windows-MCP, UFO², và mã nguồn của rcli.
- `ai-device-level-e2e-testing.md`, viết tắt **[DEV]**: README và docs của các tool desktop/mobile/terminal, cùng paper benchmark.
- `open-source-computer-use.md`, viết tắt **[CLI]**: README của các CLI computer-use.
- `ai-visible-e2e-testing.md`, viết tắt **[WEB]**: docs của Playwright và các tool web.

Độ tin của bằng chứng:
- **code**: đã đọc trong mã nguồn, có `file:line` ở file gốc.
- **README**: tác giả tự mô tả, chưa kiểm trong code.
- Chỗ nào code bác bỏ README thì theo code (xem [SRC] §9).

---

## 0. Điểm mạnh rcli phải giữ (không công cụ nào khác có)

- Điều khiển được màn khoá, màn đăng nhập và UAC: helper chạy SYSTEM trong session console, có `OpenInputDesktop` + `SetThreadDesktop` và `SendSAS`. Cua, Peekaboo và Windows-MCP **chỉ phát hiện** màn khoá rồi báo lỗi hoặc tắt. **Mức tin: code**, [SRC] §6.6 và bảng §7.
- Kiến trúc service root/SYSTEM cộng session helper, chạy được từ service mà không cần user mở app. **Mức tin: code**, [SRC] §6.6.
- Các tính năng mức fleet: `power`, `wake`, Control Lock, audit, nhiều Device sau một Server. **Mức tin: code**, [DEV] §0a.
- OCR fallback (Windows.Media.Ocr, Vision, tesseract). Nhờ nó, các nâng cấp chờ hay kiểm theo text vẫn chạy được ở màn khoá. **Mức tin: code**, [SRC] §6.1.

Mọi nâng cấp dưới đây phải giữ được các điểm này: chạy trong helper và không đòi user đã đăng nhập, trừ khi ghi khác.

---

## 1. Sửa trước: `exec` trên Windows luôn chạy bằng SYSTEM (bug phân quyền)

- `shell()` bỏ qua `_run_as` và chạy `powershell.exe` bằng chính process service (`crates/agent/src/windows_platform.rs:344-379`). **Mức tin: code**, [SRC] §6.10.
- Hậu quả:
  - `root: never` không có tác dụng.
  - `ExecResult.as_root` và audit báo `false` dù lệnh chạy SYSTEM.
  - Testkit chạy trên FakePlatform nên không bắt được lỗi này.
- Lịch sử: ticket 22 hẹn sửa ở "ticket 26", nhưng không có ticket nào làm. Comment `shortcut:` trong code đã lỗi thời.
- Đã có sẵn nửa phần sửa: `spawn_as_console_user` (`crates/agent/src/windows_session.rs:289-332`). Còn thiếu:
  - pipe stdout/stderr,
  - chờ và lấy exit code,
  - kill cây process khi hết giờ.
- Còn một quyết định thiết kế: Terminal User không phải user đang ở console. `WTSQueryUserToken` không dùng được trong trường hợp này, còn `LogonUser` cần mật khẩu, trái ADR 0005.
- Kích thước: **M**. Nên đi qua `/diagnosing-bugs` hoặc một ticket riêng.

---

## 2. Danh sách nâng cấp, xếp theo giá trị cho E2E do agent điều khiển

Thứ tự dựa trên nguyên tắc: kiểm tất định trước → chờ đúng lúc → hành động đáng tin → để lại bằng chứng → bộ khung test → mở rộng phạm vi.

| # | Nâng cấp | Rcli hiện có | Học từ ai | Mức tin | Cỡ | Phụ thuộc |
|---|---|---|---|---|---|---|
| A1 | **Mark có state, value, phần tử cha** | Mark phẳng 8 field | agent-desktop, Windows-MCP, Cua | code | M | — |
| A2 | **`wait_for` / `verify` chạy trên Agent** | chỉ `wait_ms` ≤ 5 s | Cua `verify_state`, Peekaboo `verify` | code | M | A1 (cho state/value) |
| A3 | **Action semantic kèm `effect`** | chỉ input giả lập | agent-desktop (chuỗi action), Cua (`effect`) | code | L | A1, A7 |
| A4 | **Ghi lại run: artifact từng bước + trajectory** | MCP không ghi ra đĩa | Cua `turn-NNNNN/`, UFO JSONL + `output.md` | code | S–M | — |
| A5 | **`exec` trong phiên GUI của user** | Win: SYSTEM session 0; mac: LaunchDaemon; Linux: thiếu env phiên | chính rcli (`session_env`, `spawn_as_console_user`) | code | M | mục 1 |
| A6 | **Liệt kê, focus cửa sổ, mở app** | không có | agent-desktop, Windows-MCP, Peekaboo, Cua | code | M | A5 (mở app bằng token user) |
| A7 | **Ref ổn định cho phần tử** (sống qua nhiều Observation) | Mark chết theo Observation (`stale_mark`) | agent-desktop (đường dẫn con + role + pid), Peekaboo (chấm điểm) | code | M | A1 |
| A8 | **Mở rộng phạm vi cây và thêm giới hạn** | Win: chỉ foreground, không giới hạn node hay timeout | Windows-MCP (popup/taskbar/modal, timeout 2 s/5 s), agent-desktop (`SMTO_ABORTIFHUNG`) | code | S | — |
| A9 | **Video cả kịch bản** | clip ≤ 5 s | Cua `recording.mp4`, Peekaboo `capture.mp4` | code | M | A4 |
| A10 | **Runner kịch bản + verdict** | không có | OSWorld (oracle script theo task), Maestro (YAML + JUnit), UFO EvaluationAgent | README (+code UFO) | M–L | A2, A4 |
| A11 | **Report đọc được bởi agent và CI** | không có | Maestro JUnit/HTML, tui-test `trace.md`, Playwright `error-context.md` | README | S | A4 |
| A12 | **Capture macOS bằng ScreenCaptureKit** | `CGDisplayCreateImage` (deprecated) | Cua, Peekaboo | code | S–M | — |
| A13 | **Terminal/TUI qua MCP** | MCP không có PTY | tui-test (`expect text`, snapshot, `.cast`), VHS `Wait /regex/` | README | M | — |
| A14 | **Ảnh chi tiết hơn: zoom vùng** | cạnh dài 1280, JPEG q70 | Claude `computer_toolset` có `zoom`, ảnh tối đa 2576 px | README | S | — |
| A15 | **Mobile qua Device làm host** | không có | mobile-mcp, Maestro, `adb` + `uiautomator dump` | README | L | A5 |

### A1. Mark có state, value, phần tử cha
- **Windows:**
  - Giữ `FindAllBuildCache`, nhưng thêm vào CacheRequest: `IsEnabled`, `HasKeyboardFocus`, `IsPassword`, `Value.Value`, `Toggle.ToggleState`, `ExpandCollapse.ExpandCollapseState`, `SelectionItem.IsSelected`, `RangeValue.Value`, `LegacyIAccessible.State/Value`, `RuntimeId`. Không tốn thêm round-trip.
  - Lấy quan hệ cha-con bằng `BuildUpdatedCache` + `GetCachedChildren`, như Cua.
- **macOS:** mỗi node gọi một lần `AXUIElementCopyMultipleAttributeValues`, thêm `AXEnabled/AXFocused/AXSelected/AXExpanded/AXIdentifier/AXValue`.
- **Linux:** `GetState` đã có sẵn. Chỉ cần giữ thêm các bit CHECKED/ENABLED/FOCUSED/SELECTED/EXPANDED và đọc `Value`/`Text`.
- **Chỗ cắm:** `UiElement` (`platform.rs:72-79`), `Mark` (`agent.proto:314-323`), `marks::to_mark`. Wire JSON của helper dùng serde nên field mới tự đi qua.
- **Lý do đứng đầu:** đây là thứ biến "agent đoán từ ảnh" thành "assert tất định", ví dụ "checkbox X đang tick", "ô Y có giá trị Z".

### A2. `wait_for` / `verify`
- **Thiết kế chung của Cua và Peekaboo:**
  - poll 100 ms, cần 2 mẫu ổn định liên tiếp, timeout mặc định 5 s và tối đa 10 s;
  - vị từ `exists | value_equals | enabled | selected` trên selector `{role, label_contains}`.
- **Riêng rcli:** chờ theo `text` qua OCR ở màn khoá.
- **Chỗ cắm:** thêm `WaitFor` vào `oneof kind` của `Action`, xử lý trong `Executor::step`. Phải nới `MAX_TOTAL_WAIT_MS` và timeout 10 s của Server. Trả lỗi `wait_timeout`.

### A3. Action semantic kèm `effect`
- **Chuỗi action (theo agent-desktop):**
  - Windows: Invoke → LegacyIAccessible.DoDefaultAction, rồi Toggle, ExpandCollapse, Value.SetValue, SelectionItem.Select, ScrollIntoView.
  - macOS: AXPress → AXOpen → AXConfirm, rồi ghi AXValue.
  - Linux: Action.DoAction, EditableText.
- **Kết quả (theo Cua):**
  - `effect ∈ {confirmed, partial, unverifiable, suspected_noop, refused}`.
  - Chỉ `confirmed` khi đọc lại được thay đổi: `after == expected && after != before` (value, a11y hoặc cửa sổ đổi).
  - `retry: safe` chỉ khi action chưa được giao (agent-desktop).
- **Chỗ cắm:** `Platform::perform`, op `Perform` trong `session::Request` (vì phải chạy trong helper), field `effect` trong `ActResult`, action MCP `press|set_value|toggle|select`.
- **UNVERIFIED:** helper Windows chạy SYSTEM. Chưa rõ nó gọi UIA pattern vào app của user có được không. Cần probe trước khi làm.

### A4. Ghi lại run
- **Chỉ làm phía Controller** (`rcli mcp`), không đụng Agent, và chỉ khi bật tay. Docs nói audit Server không lưu ảnh, nên giữ đúng chính sách đó.
- **Cấu trúc:** `run/step-NNN/{request.json, result.json, display-N.jpg, marks.json}` cộng `run.jsonl`. Dùng lại `save_observation` (`cli/src/lib.rs:410-437`) trong `Mcp::call`.
- **Tham khảo cấu trúc:**
  - Cua: `turn-NNNNN/{action, before.png, after.png, before_state, after_state, evidence}.json` cộng `session.json`.
  - UFO: JSONL → `output.md`.

### A5. `exec` trong phiên GUI
- **Linux:** gắn `session_env(target)` (`linux/session.rs:282-297`) vào `sudo … env …` khi Terminal User trùng user của phiên.
- **macOS:** thêm op `Shell` trong helper Aqua, hoặc dùng `launchctl asuser <uid>`.
- **Windows:** như mục 1.
- **Giá trị:** mở đường cho các CLI ngoài (agent-desktop, Cua Driver, Peekaboo, tui-test) chạy được qua `rcli exec`. Đây là cách rẻ nhất để có A1/A3/A6 tạm thời, trong khi chưa tự làm.
- **UNVERIFIED:**
  - Quyền TCC của tool con trên macOS.
  - DACL pipe của Cua với client SYSTEM.
  - `env_reset` của sudo.

### A6. Cửa sổ và app
- **Windows:**
  - Liệt kê: `EnumWindows` + `DWMWA_CLOAKED`, bỏ cửa sổ `WS_EX_TOOLWINDOW`.
  - Focus: `AllowSetForegroundWindow` + `AttachThreadInput` + `SetForegroundWindow`, rồi poll `GetForegroundWindow` để xác nhận.
  - Mở app: `Get-StartApps` / `shell:AppsFolder`.
- **macOS:** `CGWindowListCopyWindowInfo` + `NSRunningApplication.activate` + `AXRaise`.
- **Linux X11:** `_NET_CLIENT_LIST` / `_NET_ACTIVE_WINDOW`. Mở app qua `.desktop Exec`.
- **Chỗ cắm:** `Platform::{windows, focus_window, launch}`, op trong helper, tool MCP `windows` hoặc action `focus`.

### A10–A11. Runner, verdict và report
- **Kịch bản:** Markdown/YAML. Mỗi bước gồm hành động (ngôn ngữ tự nhiên) và **oracle**, theo mẫu `config/evaluator` của OSWorld. **Mức tin: README/paper**, [DEV] §3a.
- **Thứ tự ưu tiên oracle:**
  1. `exec` (file, registry, process, log),
  2. `device.session`,
  3. Mark có state/value (sau A1),
  4. LLM nhìn ảnh, chỉ khi hết cách. LLM phải trả `{pass, reason}` có trích ảnh, giống `{reason, sub_scores, complete}` của UFO EvaluationAgent. **Mức tin: code**, [SRC] §5.3.
- **Report:** JUnit cho CI, `summary.md` cho agent đọc trước (cách của tui-test `trace.md` và Playwright `error-context.md`).
- **Lý do không xếp cao hơn:** chưa có số đo độ chính xác của LLM-judge trên desktop ([DEV] §7). Các benchmark nghiêm túc đều chấm bằng script.
- **Lựa chọn thay vì tự làm:** bọc rcli thành thiết bị Midscene qua `AbstractInterface` để có YAML và report HTML. Đánh đổi:
  - chỉ nhìn ảnh,
  - danh sách model không có Claude,
  - `aiAssert` không cache.

  **Mức tin: README**, [DEV] mục "Phù hợp" §2.

### A13. Terminal/TUI
- **Hiện tại:** MCP chỉ có `exec` chạy một lần.
- **Hướng rẻ:** chạy `tui-test` trên Device qua `exec`. Nó có `expect text`, snapshot và `trace.md/json`, `.cast`. Hướng này cần A5 nếu TUI phải chạy trong phiên user.
- **Hướng sâu:** tool MCP `shell` có `wait_for` regex trên output, giống VHS `Wait /regex/`.

---

## 3. Không nên làm

| Ý tưởng | Lý do | Nguồn |
|---|---|---|
| LLM tự chấm bên trong Agent/Server | Thuộc runner phía Controller (A10). Chưa có số đo độ chính xác | [SRC] §8, [DEV] §7 |
| Input nền bằng API private macOS (`SLEventPostToPid`, `_AXUIElement*`) | Gãy theo bản macOS. Peekaboo, agent-desktop và Cua đều đang chịu rủi ro này | [SRC] §8 |
| Hook `WH_*_LL`, telemetry | Ngoài nhu cầu, tăng bề mặt tấn công | [SRC] §8 |
| Dựa vào OmniParser làm perception mặc định | Weights AGPL. Chỉ cân nhắc cho màn khoá khi OCR không đủ | [SRC] §1.9, [DEV] §2f |
| Phụ thuộc UI-TARS, Shortest, Magnitude, Octomind, SikuliX | Archived hoặc ngừng phát triển | [DEV], [CLI] "Đã ngừng" |
| Cho LLM "nhìn" live view trong test Dashboard | FakePlatform phát khung một màu, check `videoWidth`/fps đã mạnh hơn | [WEB] |

---

## 4. License

Mọi nguồn tham khảo ở mục 2 là MIT (Cua, Peekaboo, AXorcist, Windows-MCP, UFO) hoặc Apache-2.0 (agent-desktop), nên đều dùng lại được trong rcli (AGPL-3.0):
- MIT: giữ notice.
- Apache-2.0: giữ LICENSE/NOTICE, ghi rõ file đã sửa.

Không có xung đột ([SRC] §8).

---

## 5. Câu hỏi phải probe trước khi chọn

1. Helper SYSTEM của rcli có gọi được UIA pattern vào app của user không? Quyết định A3 trên Windows.
2. Tool con chạy từ helper Aqua hay qua `sudo -u` có được quyền TCC (Screen Recording, Accessibility) không? Quyết định A5 trên macOS.
3. Với Terminal User khác user console trên Windows, chọn S4U, yêu cầu user đăng nhập, hay từ chối? Đây là quyết định thiết kế, cần ADR.
4. `ScreenCaptureKit` có chạy ở LoginWindow session không? Quyết định A12.
