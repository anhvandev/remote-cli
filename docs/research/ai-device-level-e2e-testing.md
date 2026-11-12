# E2E test theo kịch bản cho AI agent ở mức cả máy (desktop, OS, terminal, mobile): các giải pháp hiện có (2026-11)

Ngày tra cứu: 2026-11-12. Mọi nguồn được đọc trong ngày này.
Phạm vi: công cụ cho AI agent chạy kịch bản E2E trên **cả máy**: app desktop native (Windows, macOS, Linux X11/Wayland), chính OS (màn khoá, màn đăng nhập, hộp thoại, settings), app terminal/CLI, và phụ là mobile. Web-only đã có ở `docs/research/ai-visible-e2e-testing.md` và không lặp lại ở đây.

Nguồn chính:
- Repo này tại commit `5fb386e` (2026-11-12, `chore(release): v0.2.3`): `crates/cli/src/mcp.rs`, `crates/agent/src/{platform,marks,observer,clip,coords,macos,windows_platform}.rs`, `crates/agent/src/linux/{mod,atspi}.rs`, `crates/protocol/proto/agent.proto`, `crates/testkit/src/lib.rs`, `crates/testkit/tests/mcp.rs`, `docs/usage.md`, `GLOSSARY.md`, `docs/adr/0001..0007`.
- Docs của nhà cung cấp model: platform.claude.com (computer use tool), developers.openai.com (computer use), aistudio.google.com/docs/computer-use.
- README và docs trong repo GitHub (đọc qua `gh api repos/<r>/readme`). Thời điểm push và release lấy từ GitHub API (`pushed_at`, `releases/latest`, `archived`). Version lấy từ npm registry và PyPI JSON.
- Paper: OSWorld (arXiv 2404.07972), OSWorld 2.0 (arXiv 2606.29537), WindowsAgentArena (arXiv 2409.08264), UFO² (arXiv 2504.14603), AgentRewardBench (arXiv 2504.08942).

Không chạy công cụ nào, không build gì. Số benchmark do chính tác giả công cụ công bố được ghi là "tự công bố". Claim nào chưa xác nhận được thì ghi **UNVERIFIED**.

---

## 0. rcli hôm nay như một harness cho test mức thiết bị

### 0a. Tool của `rcli mcp` (rmcp 3.8, stdio; `crates/cli/src/mcp.rs`)

| Tool | Tham số | Trả về |
|---|---|---|
| `devices` | — | `{devices: [...]}` kèm Device Status |
| `device` | `device` | chi tiết, inventory, tài nguyên hiện tại, `session` (`Active`/`LockScreen`/`LoginScreen`, theo `docs/usage.md`) |
| `lock_acquire` / `lock_release` / `lock_status` | `device` | JSON của Control Lock. `lock_acquire` còn mở stream `lock/watch`, nên lock được nhả sau 60 s nếu tiến trình MCP chết (`mcp.rs:31-35, 139-146`) |
| `observe` | `device`, `observe` = `shot`\|`frames`\|`clip`, `display` (số hoặc `"all"`), `marks`, `max_frames` (1–30) | ảnh inline (`ContentBlock::image`), clip dạng blob `video/mp4` (`rcli://clip.mp4`), phần JSON còn lại ở dạng structured: `observation` id và bảng Mark (`mcp.rs:113-127`) |
| `act` | `device`, `action`, cộng các tham số observe (thêm `none`) | như `observe`; cần Control Lock |
| `batch` | `device`, `actions[]` (gồm cả `wait`) | một Observation cho cả batch; lỗi thì có `failed_step` (`agent.proto` `ActResult`) |
| `exec` | `device`, `command`, `as: "root"`, `timeout_secs` (1–3600, mặc định 60) | `exit_code`, `stdout`, `stderr`, `timed_out` |
| `wake` | `device` | Wake-on-LAN qua Wake Relay |
| `power` | `device`, `action` = `restart`\|`shutdown`\|`sleep`\|`lock` | kết quả JSON |

- **Action** (`mcp.rs:43-47`): `move|click|double_click|right_click`, `drag` (`to`), `scroll` (`dx`, `dy`), `type` (`text`, `secret`, `into`), `key` (`"ctrl+c"`), `wait` (`ms`). TARGET nhận ba dạng: `x,y` theo pixel ảnh (có `display`), `mark` + `observation`, hoặc `text`.
- **Không có trong MCP** (CLI có): `displays`, `device processes`, `shell` tương tác, `audit`, `metrics history` (`docs/usage.md`: "Chưa có tool `shell` trong MCP; dùng `exec`").

### 0b. Agent nhìn thấy gì
- **Ảnh:** JPEG, cạnh dài 1280 (`coords.rs:6`, `LONG_EDGE`). Agent chờ màn hình đứng yên tối đa 1 s, poll mỗi 100 ms (`observer.rs:18-20`). Nếu ảnh giống ảnh trước đó của cùng Controller thì trả `unchanged: true` và bỏ trống `data` (`agent.proto` `Image`).
- **`frames`:** các frame có thay đổi, kèm `at_ms`, tối đa 30. **`clip`:** MP4 H.264 10 fps, dài tối đa 5 s (`clip.rs:19-22`). Theo `docs/usage.md`, CLI chỉ có clip ở `rcli act` chứ không có ở `rcli batch`. Schema MCP vẫn cho `batch` nhận `clip`.
- **Mark = cây accessibility đã làm phẳng, OCR làm dự phòng** (`marks.rs:12-24`). Mỗi Mark chỉ có `number, text, role, display, x, y, width, height` (`agent.proto` `Mark`). Không có cấu trúc cây, state (checked/enabled/focused) hay value riêng.
  - Windows: UIA `FindAllBuildCache(TreeScope_Subtree)`, **chỉ trên cửa sổ foreground** (hoặc cửa sổ ở giữa display). Chỉ cache `ControlType`, `Name` và `BoundingRectangle`, lọc `IsOffscreen=false` (`windows_platform.rs:168-203`). Role giữ lại: button, field, link, text, checkbox, radio, combobox, item, menuitem, tab (`:635-649`).
  - macOS: AX API, **chỉ app đang focus** (`AXFocusedApplication`), tối đa 3000 node, timeout 0.5 s. Text ghép từ `AXTitle/AXDescription/AXValue/AXPlaceholderValue`. Cần `AXIsProcessTrusted` (`macos.rs:163-230`).
  - Linux: AT-SPI qua accessibility bus, đi toàn bộ cây từ registry root. Chỉ lấy node `SHOWING`+`VISIBLE`, tối đa 3000 node, timeout 500 ms. Agent bật `org.a11y.Status.IsEnabled` để toolkit bắt đầu xuất cây, nhưng app mở trước đó có thể không xuất (`linux/atspi.rs`). Dùng chung cho X11 và Wayland (`linux/mod.rs:190-195`).
  - OCR: Windows.Media.Ocr (`windows_platform.rs:205`), Apple Vision `VNRecognizeTextRequest` (`macos.rs:426`), tesseract CLI (`linux/mod.rs:197-213`, có ghi chú `shortcut:`).
  - Ở màn khoá và màn đăng nhập thì "no tree here (lock/login screen, Wayland, games): the Agent falls back to OCR" (`platform.rs:18-20`). Trên Windows ở màn khoá, OCR chạy bằng profile của SYSTEM, nên có thể phải cài gói ngôn ngữ OCR (`docs/usage.md`).

### 0c. Agent hành động thế nào
- **Chỉ có synthetic input**, không có accessibility action (không có Invoke hay `AXPerformAction`). Các primitive `Input` gồm `Move/Press/Scroll/Text/Key/KeyDown/KeyUp/Sas` (`platform.rs:79-98`). Bên dưới dùng enigo/scrap của RustDesk (ADR 0002). Click theo Mark hay text chỉ đổi thành tọa độ tâm của Mark (`marks.rs:57-60`).
- **Shell:** `exec` một lần theo Terminal User hoặc root/SYSTEM, theo Device Settings (ADR 0004).
- **Mức OS:** chạy được ở màn khoá, màn đăng nhập và UAC secure desktop trên Windows. Ctrl+Alt+Del đi qua `SendSAS` và cần policy `SoftwareSASGeneration`. Màn đăng nhập macOS **chỉ xem được**, vì key injection không tới được ô mật khẩu (ADR 0001, probe trên macOS 15.8.1, 2026-11-04). Linux có cả X11 và Wayland (uinput).
- `power` (restart/shutdown/sleep/lock) và `wake` cho phép viết kịch bản boot → login → app, thứ các công cụ chạy trong phiên user không làm được.

### 0d. Artifact hiện có
- MCP trả ảnh và clip inline cho agent đang gọi, **không ghi ra đĩa**. CLI có `--out <dir>` (ảnh, `clip.mp4`) và `--json` (`docs/usage.md`).
- Audit trên Server giữ 90 ngày: ai, lúc nào, Device nào, tham số, kết quả, exit code. Docs ghi: "ảnh, clip và phím gõ trong `shell` không bao giờ được lưu" (`docs/usage.md`).
- Lỗi có code đọc được: `control_lock_held`, `stale_mark`, `ambiguous_text`, `text_not_found`, `failed_step`.

### 0e. Testkit
- `TestSystem` dựng Server thật trên Postgres testcontainer, Agent chạy trên FakePlatform, cùng `ManualClock`. Có client MCP thật qua `mcp_as`, `mcp_in`, `mcp_user` (`crates/testkit/src/lib.rs:1-5, 284-316`). `tests/mcp.rs` có kịch bản "an agent lists devices, takes the lock, acts and observes with marks".
- FakePlatform: frame một màu, đổi theo session và input (`platform.rs:309-330`). `accessibility` và `recognize_text` trả dữ liệu đã script sẵn (`:338-345`). Testkit kiểm được giao thức, nhưng **không** kiểm được việc một model nhìn ảnh thật.

### 0f. Khoảng trống khi dùng làm harness E2E
1. **Không có cây accessibility đầy đủ.** Chỉ có danh sách Mark phẳng, không cha-con, không state hay value (Windows thậm chí không lấy `Value`), và chỉ lấy cửa sổ foreground (Windows) hoặc app đang focus (macOS). Vì vậy không assert tất định kiểu "checkbox X đang tick" được, chỉ dựa vào ảnh hoặc text.
2. **Không có accessibility action và không xác nhận hiệu ứng.** So sánh: Cua Driver trả `effect: confirmed|unverifiable|suspected_noop|…` (mục 2e).
3. **Không có artifact bền:** không có thư mục run, không có step log kèm ảnh mỗi bước, không có video cả kịch bản (clip tối đa 5 s), không có report JSON/JUnit. Audit không lưu ảnh.
4. **Không có `wait_for`** (chờ tới khi text hoặc Mark xuất hiện). Chỉ có `wait` cố định (tổng ≤ 5000 ms mỗi batch) và settle ≤ 1 s, nên agent phải tự poll.
5. **Không có runner và verdict:** không có định dạng kịch bản, không có pass/fail, không có cơ chế chấm. Thứ có sẵn là các oracle tất định: `exec` (đọc file, registry, log, process), `device` (`session`), và text của Mark.
6. Không có mobile (Android/iOS).
7. `exec` chỉ chạy một lần, không có PTY trong MCP. App TUI phải test qua Live View/`shell` của CLI, hoặc chạy một harness terminal trên Device qua `exec` (mục 6).

---

## 1. Model và API computer-use

### 1a. Claude computer use (Anthropic)
- **Version tool:** `computer_toolset_20260801` không cần beta header. Docs ghi "Claude Fable 5.2, Claude Mythos 5.2, Claude Opus 5.5, Claude Sonnet 5.5, and Claude Haiku 5.5 support computer use through the `computer_toolset_20260801` toolset" và các model này "return a 400 error for any computer use tool version they don't accept". Opus 4.5–4.7 và Sonnet 4.6 chỉ nhận `computer_20251124` (header `computer-use-2025-11-24`) (https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool).
- **Member tool (17):** `screenshot`, `zoom`, `left_click`, `right_click`, `middle_click`, `double_click`, `triple_click`, `left_click_drag`, `mouse_move`, `left_mouse_down/up`, `cursor_position`, `scroll`, `type`, `key`, `hold_key`, `wait` (cùng trang).
- **Agent thấy gì:** chỉ screenshot, tọa độ "in screenshot pixels". Không có cây accessibility. Mỗi ảnh tốn khoảng "1,000–1,800 input tokens". Ảnh tối đa 2576 px cạnh dài. Trên 20 ảnh mỗi request thì có giới hạn chặt hơn (cùng trang).
- **Hành động:** app của bạn tự thực thi mọi call, nên đây chỉ là giao thức. Thay `xdotool` bằng `rcli act` là cùng mô hình.
- **Pass/fail:** không có sẵn. Docs khuyên prompt: "After each step, take a screenshot and carefully evaluate if you have achieved the right outcome…" (cùng trang).
- **Reference implementation:** `anthropics/claude-quickstarts/computer-use-demo`. Docker Linux có X11 + VNC (cổng 6080/8080/5900). Tool gồm `computer.py` (xdotool, gnome-screenshot/scrot), `bash.py`, `edit.py`. README ghi mặc định dùng `claude-opus-4-8`. Commit mới nhất vào thư mục là `12349bb` (2026-10-29). Không có runner test và không có verdict (README của demo).
- **Benchmark:** OSWorld 2.0, Claude Opus 4.8 (max thinking, 500 step): 20.6% hoàn thành, 54.8% partial (arXiv 2606.29537, 2026-06). Không có số chính thức cho Claude 5.x trong các nguồn đã đọc: **UNVERIFIED**.

### 1b. OpenAI computer use
- Tool `computer` trong Responses API. Action: `click`, `double_click`, `drag`, `move`, `scroll`, `keypress`, `type`, `wait`, `screenshot`. Ví dụ dùng `gpt-6.1-sol`. Với `gpt-6-astra`, trang **khuyên dùng code execution** thay vì tool computer, và ghi `computer` "remains supported as an alternative". Có mục "Migration from computer-use-preview" (https://developers.openai.com/api/docs/guides/tools-computer-use).
- Model chỉ nhận screenshot. Trang không nêu OS nào được hỗ trợ.
- Sample app `openai/openai-cua-sample-app` (push 2026-10-19) có hai bản: JS/Playwright cho browser và Python/PyAutoGUI cho desktop. Artifact: "Workspaces, screenshots, and replays stay in … `data/`", có **Replay JSON** và screenshot timeline. README ghi rõ: "the answer alone does not prove success" (README).
- Operator/ChatGPT agent là sản phẩm của ChatGPT, không phải API cho harness. Trạng thái hiện tại: **UNVERIFIED** (chưa đọc nguồn chính).
- OSWorld 2.0: GPT-5.5 "plateaus near 13%" (arXiv 2606.29537).

### 1c. Gemini computer use
- Tool `computer_use` có `environment` là `browser`, `mobile` ("Android-optimized") hoặc `desktop`. Desktop có các action của browser trừ `navigate/go_back/go_forward`. Mobile có `open_app`, `list_apps`, `long_press`, … Tọa độ được chuẩn hoá về 0–999 (https://aistudio.google.com/docs/computer-use).
- Model: `gemini-4-carbon`, `gemini-4-argon`, `gemini-3.8-flash` ("The recommended model for computer use"), `gemini-3.5-flash-lite`, `gemini-3-flash-preview`. Có `safety_decision` → `require_confirmation` (cùng trang).
- Reference `google-gemini/computer-use-preview` chỉ dùng Playwright/Browserbase (README, push 2026-07-28), chưa có ví dụ desktop.

### 1d. UI-TARS / UI-TARS-desktop (ByteDance)
- **Cả `bytedance/UI-TARS` và `bytedance/UI-TARS-desktop` đã archived** (GitHub API, 2026-11-12). Release cuối của desktop là v0.3.1 (2026-10-08), chỉ để vá bảo mật cho Agent TARS CLI server.
- Model chỉ dùng screenshot (native GUI agent). README tự công bố UI-TARS-1.5 (2025-04): OSWorld 42.5 (100 step), ScreenSpot-V2 94.2, ScreenSpot-Pro 61.6 (README `bytedance/UI-TARS`).
- Weights UI-TARS-1.5-7B vẫn được dùng làm grounding model cho Agent S3 (mục 1e) và có trong danh sách model của Midscene.

### 1e. Agent S (Simular)
- `gui-agents` 0.3.2 (PyPI, 2025-12-16). Repo push 2026-10-30. Chạy trên macOS, Windows và Linux, "looks at the screen and completes the task by clicking, typing, and scrolling" (README).
- **S3:** model chính cộng grounding model bắt buộc (khuyên dùng `gpt-5-2025-08-07` + UI-TARS-1.5-7B). Hành động qua `pyautogui`. `--enable_local_env` cho chạy Python/Bash, với cảnh báo "executes arbitrary Python and Bash code" (`gui_agents/s3/README.md`).
- Kịch bản là một câu lệnh ngôn ngữ tự nhiên, không có verdict cho người dùng. Behavior Best-of-N chọn trajectory tốt nhất trong N lần chạy, nghĩa là mỗi task chạy N lần.
- Số tự công bố: S3 đạt 66% OSWorld ở 100 step, 72.6% khi có bBoN (2025-12) (s3 README). S4 (2026-10-20) đạt 21.3% OSWorld V2 với Qwen3.8-27B; README S4 cũng ghi GPT-5.5 13.0% và Claude Opus 4.8 20.6% (`agent_s4/README.md`).

---

## 2. Tự động hoá desktop cho agent

### 2a. Microsoft UFO² / UFO³
- `microsoft/UFO` v3.0.10 (2026-09-22), push 2026-10-30. UFO² là "Desktop AgentOS" chỉ cho Windows. UFO³ Galaxy điều phối nhiều thiết bị Windows, Linux, Android (README).
- **Agent thấy gì:** "Windows UIA, Win32, WinCOM native control" cùng "hybrid UIA + vision pipeline". **Hành động:** "GUI clicks + API calls" (README `ufo/README.md`).
- **Pass/fail:** `EvaluationAgent` "is fully LLM-driven and conducts evaluations based on action trajectories and screenshots. Since LLM-based evaluation may not be 100% accurate, the results should be used as guidance". Nó tách sub-goal, cho ra yes, no hoặc **unsure**. `EVA_ALL_SCREENSHOTS=False` thì chỉ dùng ảnh đầu và ảnh cuối (`documents/docs/ufo2/evaluation/evaluation_agent.md`).
- **Artifact:** `logs/{task}/request.log`, `response.log` (step log), `evaluation.log`, screenshot mỗi bước, `ui_tree/step_N.json` (cây UIA đầy đủ có `control_type` và `rectangle`, bật bằng `SAVE_UI_TREE`) (`documents/docs/ufo2/evaluation/logs/*.md`).
- Paper UFO² (arXiv 2504.14603, 2025-04) không đưa số success rate trong abstract. Docs có hướng dẫn chạy trên WAA (154 task) và OSWorld-Windows (49 task).

### 2b. Windows-MCP (CursorTouch)
- PyPI `windows-mcp` 0.8.8 (2026-10-10). Repo push 2026-11-12. Hỗ trợ Windows 10/11, Python 3.14+ (README).
- **Tool:** `Click`, `Type`, `Scroll`, `Move`, `Shortcut`, `Wait`, `WaitFor` (chờ text, cửa sổ hoặc element), `ControlStatus`, `DisplayInventory`, `Screenshot`, `Snapshot` (cây UI có element id, `use_vision`, `use_dom=True`), `App`, `PowerShell`, `FileSystem`, `Scrape`, `MultiSelect`, `MultiEdit`, `Clipboard`, `Process`, `Notification`, `Registry` (README, mục MCP Tools).
- Agent thấy cây a11y UIA (thư viện Python-UIAutomation), có ảnh nếu bật. Hành động bằng tọa độ và input giả lập.
- README tự nêu giới hạn: XAML shell (Start, taskbar) "ignore synthesized mouse input" khi drag. Telemetry bật mặc định (`ANONYMIZED_TELEMETRY`).
- Cài dưới dạng **Scheduled Task theo user** (`windows-mcp install`). Theo cách cài này thì nó không thấy màn khoá hay UAC secure desktop; đây là suy luận, chưa kiểm: **UNVERIFIED**. Không có verdict và không có report.

### 2c. MCP cho macOS dùng AX API
- **CursorTouch/MacOS-MCP** v0.4.7 (2026-11-02), macOS 12–15. Dùng Accessibility API cùng Screen Recording. Tool: `Click`, `Type`, `Scroll`, `Move`, `Shortcut`, `App`, `Desktop`, `Shell` (gồm `osascript`), `Scrape`, `Wait`. README còn nhắc tool `Snapshot` (README).
- **Peekaboo** (`openclaw/Peekaboo`, trước là steipete) 4.9.8 (2026-11-05), macOS 15+. Là CLI, menu-bar app và MCP. `peekaboo see --app Finder --json` trả "a structured UI map with opaque element IDs", sau đó `click "<label>"`, `type`, `press`. Input đi theo cách "background delivery" khi resolve được process (README).
- `steipete/macos-automator-mcp` (AppleScript/JXA) **đã archived**.
- Chung cho nhóm này: chạy trong phiên user đã đăng nhập và cần TCC (Accessibility, Screen Recording). Không có verdict.

### 2d. Terminator (mediar-ai)
- "Terminator currently supports **Windows only**". `terminator-mcp-agent` 0.24.28 (npm, 2026-01-28). Release GitHub v0.24.32 (2026-04-05). Repo push lần cuối 2026-06-02 (README; GitHub API).
- Thấy "pixels, DOM, and Accessibility tree". Chạy workflow YAML tất định và chỉ gọi AI "only when recovery is needed". Câu ">95% success rate" là claim marketing, không có nguồn đo (README).
- **Hoạt động chậm lại từ giữa 2026**; công ty đẩy sản phẩm Mediar IDE.

### 2e. Cua Driver (trycua)
- `cua-driver-rs-v0.47.17` (2026-11-12). Repo `trycua/cua` push 2026-11-12. Driver mang license MIT.
- **Agent thấy gì:** cây a11y native trên cả ba OS (AX trên macOS, UIA trên Windows, AT-SPI trên Linux), mỗi element có `element_token`. `get_window_state` trả cây cùng ảnh. Extension `cua-perception` tùy chọn parse ảnh thành vùng (https://cua.ai/docs/cua-driver/concepts/how-cua-driver-works). Extension này **AGPL-3.0** vì dùng icon detector của OmniParser (`libs/cua-driver/README.md`).
- **Hành động** theo bốn nấc: (1) accessibility action qua element (UIA Invoke, `AXPerformAction`, AT-SPI action), "the only rung the driver can verify itself"; (2) pixel ở background; (3) DOM qua CDP; (4) foreground. Trên Wayland, "raw background keys are refused".
- **Mỗi phản hồi có `effect`:** `confirmed`, `unverifiable`, `suspected_noop`, `partial`, `refused`. "Only accessibility read-back sets `verified` to `true`" (cùng trang).
- Tool MCP: `get_window_state`, `get_desktop_state`, `parse_visual_regions`, `click`, `type_text`, `press_key`, `hotkey`, `scroll`, `drag`, `move_cursor`, `start_session`, `end_session`. Có CLI `cua-driver call` cho agent dùng shell (README driver).
- Không có recording hay trajectory dành cho agent trong docs đã đọc.

### 2f. OmniParser (Microsoft)
- Parse screenshot thành phần tử có nhãn cho "Pure Vision Based GUI Agent". V2 ra 2025-02. 2026-07 thêm detector YOLOv9-E (`icon_detect_v3`, MIT). Các detector cũ dựa trên Ultralytics vẫn AGPL (README, mục Model Weights License). Release v.2.0.1 (2025-09-12), push 2026-07-20.
- Tự công bố: 39.5% trên ScreenSpot-Pro với OmniParser V2 (2025-01, README). OmniTool điều khiển một VM Windows 11.
- Đây là một lớp perception chứ không phải harness. Có thể dùng cho bề mặt không có cây a11y (màn khoá, game, Remote Desktop).

### 2g. Open Interpreter OS mode
- Repo `openinterpreter/openinterpreter` nay là "A coding agent for open models", viết lại bằng Rust dựa trên Codex (rust-v0.0.61, 2026-11-11). Bản Python cũ chuyển sang fork cộng đồng `endolith/open-interpreter` (37 sao).
- Phần "Computer Use" bây giờ là QA skill gọi agent-browser (web) và **trycua** (native) (README). **OS mode cũ không còn trong sản phẩm chính.**

### 2h. Khác
- **AskUI** `askui/python-sdk` (push 2026-10-26): Windows, macOS, Linux, Android qua "Agent OS" (README). Chưa đọc cách chấm pass/fail: **UNVERIFIED**.
- **OpenAdapt** (push 2026-11-12): người hoặc agent làm mẫu một lần, OpenAdapt biến nó thành chương trình local "repeats the steps with no model calls, checks each save, and stops when something doesn't match" (mô tả repo). Mẫu này gần với "record bằng agent, replay tất định".

---

## 3. Sandbox và harness chạy agent trên VM đầy đủ

### 3a. OSWorld / OSWorld-Verified / OSWorld 2.0 (XLANG)
- **OSWorld (v1):** 369 task trên Ubuntu (thêm 43 task Windows), "a custom execution-based evaluation script" cho mỗi task, tổng 134 hàm đánh giá. Người làm được 72.36%, model tốt nhất lúc ra paper (2024-04) đạt 12.24% (http://osworld-v1.xlang.ai/, arXiv 2404.07972).
- **Cách chấm (đọc từ task JSON):** `config` dựng trạng thái đầu (`download`, `launch`). `evaluator` gồm `func` (ví dụ `check_moved_jpgs`), `result` getter (ví dụ `{type: list_directory, path: …}`) và `expected` (`rules`). Task không thể làm có `func: "infeasible"` (`evaluation_examples/examples/os/*.json`, `…/gimp/045bf3ff….json`, commit `9466ba2`, 2026-11-10). **Không dùng LLM để chấm.**
- OSWorld-Verified ra 2025-07-28 (sửa task, chạy song song trên AWS). `desktop-env` 1.0.3 (PyPI, 2026-11-10). Provider: VMware, VirtualBox, Docker/KVM, AWS, Azure, Modal (README). Muốn lên leaderboard verified thì phải để nhóm OSWorld chạy agent của bạn (README).
- **OSWorld 2.0** (arXiv 2606.29537, 2026-06-26; repo `xlang-ai/OSWorld-V2`, release `osworld-v2.1` 2026-09-16): 108 workflow dài, chấm "binary-completion" kèm partial score. Task class nằm trong dataset Hugging Face có gate, "to reduce benchmark leakage" (README V2).

### 3b. WindowsAgentArena (Microsoft)
- "150+" task trên Windows 11. Navi đạt 19.5%, người không trợ giúp đạt 74.5% (arXiv 2409.08264, 2024-09). Release v0.0.4 (2024-09-28), push 2026-10-26.
- Perception chọn qua `--som-origin`: `a11y` (`--a11y-backend uia|win32`), `omni`, `mixed-omni`, `oss` (OCR Tesseract, groundingdino). Chạy song song trên Azure ML. Golden image 30 GB (README).
- Cách chấm kế thừa từ OSWorld (evaluator theo task). Chưa đọc code chấm: **UNVERIFIED**.

### 3c. trycua/cua (Sandbox, Lume, Cua Bench)
- `cua sb create ubuntu` (gVisor container), `cua sb exec`, `cua sb screenshot`. SDK có Python, TypeScript, Rust. Lume tạo VM macOS/Linux trên Apple Silicon (README `trycua/cua`).
- **Cua Bench** (`bench-v0.4.1`, 2026-11-10): "create a small task, run its reference solution, and verify that its evaluator reports a reward of `1.0`", rồi export trajectory (README).
- **Gói cũ đã deprecated:** `cua-agent`, `cua-computer`, `cua-computer-server`, `cua-som`, `cua-core`, … Riêng `cuabot`: "**Do not run it:** it has known security issues" (README, mục Deprecated packages). Code mẫu cũ dùng `cua-agent` không còn đúng.
- Cua Spaces: miễn phí cho cá nhân, Pro $18/tháng. License FSL-1.1-MIT (README).

### 3d. E2B Desktop
- `@e2b/desktop` 2.5.0 (npm, 2026-10-13). Repo push 2026-11-11. Là sandbox Linux + Xfce trên cloud E2B, có API cho mouse, keyboard, window, screenshot, `commands.run`, stream màn hình (README).
- Không có cây a11y, không có verdict. Chỉ có Linux.

### 3e. Scrapybara
- Docs: "virtual desktop infrastructure for computer use agents", gồm instance Ubuntu và Browser (https://docs.scrapybara.com/). SDK `scrapybara` 2.5.3 (PyPI, 2025-05-28). Repo SDK push lần cuối 2025-09-08.
- **Không có hoạt động công khai trong hơn một năm.** Trạng thái dịch vụ: **UNVERIFIED**.

### 3f. Anthropic computer-use-demo
- Xem 1a. Đây là container tham khảo cho một máy Linux, không phải harness đánh giá.

---

## 4. Tự động hoá cổ điển cộng lớp AI

- **Appium** 3.9.0 (npm, 2026-10-31).
  - `appium-windows-driver` (WinAppDriver) **đã archived và deprecated**, "not compatible with Appium 4". Bản thay chính thức là `appium-wincore-driver` 4.0.9 (2026-11-11), chạy UIA thật, "Built-in screen recording" (README hai repo).
  - NovaWindows driver 1.5.0 (2026-10-24) là bản thay thế từ bên thứ ba.
  - macOS: `appium-mac2-driver` 5.0.0 (2026-11-09, dựa trên XCTest).
- **WinAppDriver** v1.2.1 (2020-11). README của Appium ghi: "has not been maintained by Microsoft for years".
- **FlaUI** v5.0.0 (2025-02, push 2026-08). **pywinauto** 0.6.9 (2025-01, push 2026-11-08). Cả hai là thư viện UIA/Win32 không có AI. Đáng dùng làm oracle tất định chạy trên Device (qua `exec`).
- **SikuliX:** repo `SikuliX1` là "archived mirror". Bản tiếp nối là **OculiX** (`oculix-org/Oculix`, push 2026-11-05, MIT): nhận dạng bằng ảnh và OCR (Tesseract đi kèm), có VNC stack, runner Robot Framework, và module MCP với "Ed25519-signed, SHA-256-chained JSONL audit journal" (README).
- **Robot Framework** 7.5.1 (2026-11-02). Core không có AI. Thư viện AI bên thứ ba chưa được kiểm: **UNVERIFIED**.
- **Tricentis Tosca Vision AI:** nhận dạng control chỉ bằng hình ảnh, dành cho Citrix, VMware, RDP. Script viết bằng VisionScript. Có self-healing (https://docs.tricentis.com/tosca-2026.1/en-us/content/vision_ai/vision_ai_introduction.htm; https://vision-ai.app.tricentis.com/documentation/visionscript). Thương mại, chạy trên Windows.
- **Eggplant Functional (Keysight):** SenseTalk với `click image:`, `click text:` (OCR). Từ EPF 26.2.1.1 (2026-05) có `description:` ("Find by Description", gửi mô tả cho LLM), ví dụ `WaitFor 20, description:"The installation is complete!"`. Điều khiển SUT qua VNC/RDP (https://docs.eggplantsoftware.com/epf/epf-top-10-commands-functions/, `/epf/epf-v26-release-notes/`). Thương mại.
- **Applitools Eyes:** match level `Strict`, `Layout`, `Ignore Colors`, `Dynamic` (https://applitools.com/docs/eyes/concepts/best-practices/match-levels). SDK **Eyes Images** "uploads images for validation" (`@applitools/eyes-images` 4.33.0, 2026-11-05; PyPI `eyes-images` 6.5.20). Vì vậy có thể so baseline cho bất kỳ ảnh nào, kể cả ảnh `rcli shot`. SDK Windows desktop riêng còn được hỗ trợ hay không: **UNVERIFIED** (chỉ thấy release note 2018–2019).

---

## 5. Mobile

- **Maestro** (`cli-2.12.1`, 2026-11-02):
  - Kịch bản là YAML (`launchApp`, `tapOn`, `assertVisible`). Maestro "drive[s] the app through the platform's accessibility layer". Có `maestro mcp` gói sẵn trong CLI. Chạy Android, iOS (simulator; "Physical iOS devices are not yet supported") và web (README).
  - **`assertWithAI`** ("experimental") gửi screenshot cùng câu assertion tới LLM và nhận true/false. `optional` mặc định là `true`, nên fail không làm hỏng flow nếu không đặt `false`. Report `ai-(flow).json` và `ai-report-(flow).html` nằm trong `~/.maestro/tests`. Trang không nói model nào được dùng (https://docs.maestro.dev/api-reference/commands/assertwithai).
  - `maestro test --format JUNIT|HTML`, `--test-output-dir`, `--debug-output`. `maestro record` xuất video (https://docs.maestro.dev/maestro-cli/maestro-cli-commands-and-options).
- **mobile-mcp** (`mobile-next/mobile-mcp` 1.0.17, 2026-11-11): "Accessibility-first … falling back to screenshots + coordinates". Tool có `mobile_list_elements_on_screen`, `mobile_click_on_screen_at_coordinates` (theo tọa độ hoặc ref), `mobile_start/stop_screen_recording`, `mobile_get_device_logs`, `mobile_list_crashes`, … Chạy iOS và Android, cả máy thật (README).
- **Midscene** (1.17.8, 2026-11-11):
  - `@midscene/android` (adb), `@midscene/ios` (WDA), `@midscene/harmony`, và `@midscene/computer` cho desktop Windows/macOS/Linux "using native mouse and keyboard controls or Remote Desktop Protocol (RDP)" (https://midscenejs.com/platforms/desktop.md). Midscene chỉ nhìn ảnh.
  - **Cắm thêm thiết bị tùy ý được**: "implementing a UI operation class that conforms to `AbstractInterface`" để có SDK, YAML, playground (https://midscenejs.com/integrate-with-any-interface.md).
  - Report HTML có ảnh từng bước. `report-tool` xuất JSON/Markdown. MCP đã bị gỡ ("pin Midscene to 1.9.8") (llms.txt). Danh sách model không có Claude (xem file web).
  - AndroidWorld: tự công bố Pass@1 93.10% với `Gemini-3.5-Flash`, Midscene 1.9.5. Kèm ghi chú "A small number of AndroidWorld validators were aligned with the task intent" và có sửa ổn định môi trường (https://midscenejs.com/android-world-benchmark-report.md). Vì vậy không so trực tiếp với số AndroidWorld gốc được.
- **appium-mcp** 1.99.3 (2026-11-04): chỉ Android (UiAutomator2) và iOS (XCUITest). Tool: `appium_find_element`, `appium_gesture`, `appium_screenshot`, `appium_screen_recording`, `appium_get_page_source`, `appium_ai` (bật bằng `AI_VISION_ENABLED`; mặc định dùng `Qwen3-VL-235B-A22B-Instruct`). Sinh code Java/TestNG. `APPIUM_MCP_EVIDENCE` đính kèm "action evidence record" JSON. Có OpenTelemetry (README).

---

## 6. Test terminal/CLI cho agent

- **microsoft/tui-test** 0.1.1 (2026-10-23), push 2026-11-10:
  - "controls, inspects, tests, and records real shell sessions and full-screen terminal apps on Windows, Linux, and macOS". CLI gồm `run`, `expect text`, `click text`, `expect snapshot NAME [-u]`, `screenshot -o x.svg`, `record start x.gif|.mp4|.cast`. Có thư viện Rust, Python, JS, Go.
  - Có lệnh cho agent: `agent-context` (schema JSON), `skill --add`.
  - Trace khi fail: "Agents should read `trace.md` first and use `trace.json` and `timeline.json`". Có `trace.html` và `session.cast` để replay (README).
  - Pass/fail tất định (text, snapshot). Không có LLM-judge sẵn.
- **VHS** (`charmbracelet/vhs` v0.12.1, 2026-09-24): file `.tape` với `Type`, `Wait /regex/`, `Output out.gif|.mp4|.webm|frames/`. Dùng cho "integration testing and demoing". Không có judge (README).
- **asciinema** v3.2.1 (2026-06-16): ghi `.cast`, không có assertion.
- **tmux-mcp** (`nickgnd/tmux-mcp`, push 2026-02-14): `capture-pane`, `execute-command`, `get-command-result` (README). Không có recording hay assertion.
- Không tìm thấy công cụ terminal nào có sẵn LLM chấm pass/fail. Cách hiện có là agent đọc `trace.md` hoặc snapshot rồi tự kết luận.

---

## 7. Số benchmark (chỉ những gì đã đọc từ nguồn chính)

| Benchmark | Số | Model/agent | Ngày | Nguồn |
|---|---|---|---|---|
| OSWorld v1 | người 72.36%; tốt nhất 12.24% | (paper gốc) | 2024-04 | arXiv 2404.07972 / osworld-v1.xlang.ai |
| OSWorld (100 step) | 42.5% | UI-TARS-1.5 (tự công bố) | 2025-04 | README `bytedance/UI-TARS` |
| OSWorld | 66% / 72.6% có bBoN | Agent S3 (tự công bố) | 2025-10 / 2025-12 | `gui_agents/s3/README.md` |
| OSWorld 2.0 (500 step) | 20.6% hoàn thành, 54.8% partial | Claude Opus 4.8 max thinking | 2026-06 | arXiv 2606.29537 |
| OSWorld 2.0 | ~13% | GPT-5.5 | 2026-06 | arXiv 2606.29537 |
| OSWorld V2 | 21.3% | Agent S4 + Qwen3.8-27B (tự công bố) | 2026-10 | `agent_s4/README.md` |
| WindowsAgentArena | 19.5% (người 74.5%) | Navi | 2024-09 | arXiv 2409.08264 |
| ScreenSpot-Pro | 61.6% | UI-TARS-1.5 (tự công bố) | 2025-04 | README UI-TARS |
| ScreenSpot-Pro | 39.5% | OmniParser V2 (tự công bố) | 2025-01 | README OmniParser |
| AndroidWorld | Pass@1 93.10% (validator đã chỉnh) | Midscene 1.9.5 + Gemini-3.5-Flash | — | midscenejs.com |

- Leaderboard OSWorld-Verified không tải được (trang chỉ hiện "Loading verified benchmark data…"). Các số mới hơn mà trang thứ cấp đưa (ví dụ cho model "Fable 5.1", "GPT-6 Astra") là **UNVERIFIED** và không dùng.
- **Độ chính xác của LLM-judge:** các benchmark ở trên đều chấm bằng script hoặc validator lập trình sẵn, không dùng LLM. UFO tự nhận LLM judge "may not be 100% accurate". AgentRewardBench (12 LLM judge, 1302 trajectory web) kết luận "no single LLM excels across all benchmarks" và rule-based "tends to underreport" (arXiv 2504.08942). Chưa thấy số đo độ chính xác của LLM-judge cho trajectory desktop: **UNVERIFIED**.

---

## So sánh

| Giải pháp | Nền tảng | Perception | Hành động | Cách chấm | Artifact | Tất định | Chi phí | Bảo trì 2026-11 | Hợp với repo |
|---|---|---|---|---|---|---|---|---|---|
| **rcli mcp** (hiện tại) | Win, macOS, Linux X11/Wayland; màn khoá/đăng nhập/UAC (macOS login chỉ xem) | pixel + Mark phẳng (UIA/AX/AT-SPI → OCR) | input giả lập, `exec`, power, wake | không có; oracle qua `exec`/`device` | ảnh/clip ≤5 s inline; CLI `--out`; audit không ảnh | transport tất định; agent thì không | chỉ tiền model | v0.2.3 (repo này) | nền tảng sẵn có |
| Claude computer use | bạn tự cấp | chỉ screenshot (+zoom) | bạn thực thi | tự thiết kế | tùy bạn | không | token, ~1–1.8k/ảnh | `_20260801` | lớp model cho rcli |
| OpenAI `computer` | bạn tự cấp | screenshot | bạn thực thi; khuyên code exec | tự thiết kế | sample: replay JSON + timeline | không | token | đang hoạt động | lớp model |
| Gemini `computer_use` | browser/desktop/mobile | screenshot, tọa độ 0–999 | bạn thực thi | tự thiết kế | tùy bạn | không | token | đang hoạt động | lớp model |
| UI-TARS(-desktop) | Win/macOS/browser | screenshot | input giả lập | không | — | không | model tự host | **archived** | không |
| Agent S3/S4 | Win/macOS/Linux | screenshot + grounding model | pyautogui (+code) | không (bBoN chọn run) | — | không | 2 model | 0.3.2 (2025-12), repo hoạt động | thấp |
| UFO² / UFO³ | Windows (Galaxy: +Linux, Android) | UIA + vision | UIA/Win32/COM + GUI | LLM EvaluationAgent (yes/no/unsure) | log, ảnh mỗi bước, cây UIA JSON mỗi bước | không | model | v3.0.10 | ý tưởng artifact |
| Windows-MCP | Win 10/11 (phiên user) | cây UIA + ảnh tùy chọn | tọa độ, PowerShell | không | log server | công cụ thì có | miễn phí | 0.8.8 | chạy trên Device qua `exec` (UNVERIFIED) |
| MacOS-MCP / Peekaboo | macOS (phiên user) | AX + ảnh | AX/tọa độ, AppleScript | không | ảnh qua `see --path` | công cụ thì có | miễn phí | 0.4.7 / 4.9.8 | như trên |
| Cua Driver | Win/macOS/Linux (phiên user) | cây a11y đầy đủ + ảnh; perception AGPL tùy chọn | a11y action → pixel → CDP → foreground | `effect`/`verified` mỗi action | không có recording cho agent | action a11y thì kiểm được | miễn phí (MIT) | rất hoạt động | bổ sung a11y trên Device |
| Terminator | Windows | pixel + DOM + a11y | workflow YAML + AI recovery | không | — | phần lớn | miễn phí | chậm từ 2026-06 | không |
| OmniParser | ảnh bất kỳ | parse ảnh | — | — | — | model | GPU/CPU | 2026-07 | perception cho màn khoá (license) |
| OSWorld / 2.0 | Ubuntu VM (+Win) | tùy agent | tùy agent | **script theo task** | trajectory, kết quả | chấm tất định | VM + model | rất hoạt động | mẫu viết kịch bản/oracle |
| WAA | Windows 11 VM | a11y/Omni/OCR | tùy agent | evaluator theo task (UNVERIFIED) | log | chấm tất định | Azure | push 2026-10, release 2024 | mẫu |
| cua sandbox / Bench | Linux container, VM macOS/Linux | tùy agent | SDK | evaluator → reward | trajectory export | chấm tất định | miễn phí/Spaces | rất hoạt động | không cần (rcli đã có máy thật) |
| E2B Desktop / Scrapybara | Linux cloud | screenshot | SDK | không | stream | — | trả theo giờ | E2B hoạt động / Scrapybara im ắng | không |
| Appium (+wincore, mac2) | Win, macOS, Android, iOS | cây a11y (WebDriver) | WebDriver | assertion code | recording (wincore), log | có | miễn phí | 3.9.0 | trùng một phần, không chạy được ở màn khoá |
| Tosca Vision AI / Eggplant | Win (+VDI/VNC/RDP) | ảnh + OCR + LLM (Eggplant `description:`) | input qua VNC/RDP | assertion trong script | report thương mại | phần lớn | license | đang bán | không (đóng, đắt) |
| Applitools Eyes Images | ảnh bất kỳ | so ảnh Visual AI | — | so baseline | dashboard | gần tất định | trả phí | 4.33.0 | lớp so ảnh cho `rcli shot` |
| Maestro | Android, iOS sim, web | a11y (+ảnh cho `assertWithAI`) | a11y | assertion YAML; `assertWithAI` LLM | JUnit/HTML, ảnh, video, `ai-*.json` | có (trừ AI) | miễn phí (+cloud) | 2.12.1 | cho mobile, ngoài rcli |
| mobile-mcp | Android, iOS (cả máy thật) | a11y → ảnh | a11y/tọa độ | không | recording, log, crash | công cụ thì có | miễn phí | 1.0.17 | cho mobile |
| Midscene (Android/iOS/computer) | Android, iOS, Harmony, desktop, RDP | chỉ ảnh | input giả lập/adb/WDA | `aiAssert` (VLM) | HTML có ảnh từng bước → JSON/MD | không | model mỗi assert | 1.17.8 | có `AbstractInterface` để bọc rcli |
| tui-test | terminal Win/macOS/Linux | lưới text + snapshot | gõ phím, click text | `expect` tất định | `trace.md/json`, `.cast`, GIF/MP4 | có | miễn phí | 0.1.1 | chạy trên Device qua `exec` |
| VHS / asciinema / tmux-mcp | terminal | text | gõ phím | không (VHS có `Wait`) | GIF/MP4/cast | có | miễn phí | hoạt động | phụ |

---

## Phù hợp với repo này

Các dữ kiện quyết định:
- rcli đã làm được phần khó nhất mà công cụ ngoài không làm: chạy trên máy thật ở cả ba OS, **kể cả màn khoá, màn đăng nhập và UAC**, cùng power và wake.
- Mọi công cụ desktop-agent khác (Windows-MCP, MacOS-MCP, Peekaboo, Cua Driver, Midscene computer) chạy trong phiên user và cần quyền TCC hoặc UIPI tương ứng.
- Thứ rcli thiếu là phần "test": kịch bản, verdict và artifact. Cây a11y hiện chỉ đủ để click, chưa đủ để assert.

### 1. Dùng chính `rcli mcp` làm harness, verdict ưu tiên oracle tất định, rồi bổ sung artifact
- **Cách làm:**
  - Kịch bản là file Markdown/YAML. Mỗi bước gồm hành động bằng ngôn ngữ tự nhiên và **oracle**, theo mẫu `config/evaluator` của OSWorld (mục 3a).
  - Oracle ưu tiên `exec` (file, registry, `loginctl`, `query user`, log, process), `device.session` (`LockScreen` → `Active`) và text của Mark. Chỉ khi không còn cách nào khác mới dùng LLM nhìn ảnh, và LLM phải trả JSON `{pass, reason}` có trích ảnh.
  - Claude Code (hoặc một vòng Messages API) đóng vai agent.
- **Bước nhỏ nhất:**
  - Viết một kịch bản "khoá rồi mở khoá" cho một Device Windows test: `power lock` → `device` (kỳ vọng `LockScreen`) → `lock_acquire` → `act type secret` + `key enter` → `device` (kỳ vọng `Active`) → `exec whoami`.
  - Chạy bằng Claude Code với `claude mcp add rcli -- rcli mcp`.
  - Lưu artifact bằng CLI: `rcli act … --out run/step-N --json`. Kịch bản này không cần sửa code.
- **Khoảng trống phải tự lấp (theo thứ tự cần nhất):**
  - Ghi artifact cho mỗi tool call: request, ảnh, bảng Mark, kết quả `exec`, vào một thư mục run có step log JSON. Hiện MCP chỉ trả inline, còn audit không lưu ảnh.
  - Thêm `wait_for` theo text hoặc Mark.
  - Video cả kịch bản: clip hiện tối đa 5 s. Live View có WebRTC nhưng chưa có tool ghi.
- **Đánh đổi:**
  - Agent không tất định, mỗi bước tốn token ảnh (khoảng 1–1.8k mỗi ảnh với Claude).
  - Cây a11y phẳng nên assertion về state (checked, value) phải đi qua `exec` hoặc ảnh.
  - Màn đăng nhập macOS chỉ quan sát được (ADR 0001).
  - Testkit/FakePlatform chỉ kiểm được giao thức, không kiểm được chất lượng "nhìn" của model.

### 2. rcli làm transport, bọc thành thiết bị Midscene qua `AbstractInterface` để có runner YAML và report
- **Lý do:** có sẵn định dạng kịch bản (YAML/Midscene Test), `aiAssert`/`aiQuery`, và report HTML có ảnh từng bước mà `report-tool` xuất được ra JSON/Markdown cho agent khác đọc. Khi đó rcli không phải tự xây runner và report.
- **Bước nhỏ nhất:** clone `midscene-example/custom-interface`, cài `screenshot()` bằng `POST /api/v1/devices/{id}/…` (như `rcli shot`) và các action tap/input/key bằng `rcli act`. Sau đó chạy một YAML trên một Device Linux test.
- **Đánh đổi:**
  - Midscene chỉ nhìn ảnh, nên không dùng Mark/a11y của rcli trừ khi tự thêm action.
  - Danh sách model không có Claude, nên kéo theo nhà cung cấp model khác (Qwen, Gemini, GPT, …).
  - `aiAssert` không bao giờ được cache, mỗi lần đều gọi model.
  - Thêm Node/TypeScript vào luồng test.
  - Mapping endpoint cụ thể chưa thử: **UNVERIFIED**.

### 3. Bổ sung cây a11y đầy đủ trên Device bằng công cụ chạy trong phiên user, gọi qua `rcli exec`
- **Lý do:** lấp khoảng trống 1–2 ở mục 0f cho kịch bản trong app (không phải màn khoá).
  - Cua Driver (MIT, cả ba OS) cho cây a11y có token, accessibility action và `effect`/`verified` cho mỗi action.
  - Trên Windows có pywinauto/FlaUI làm oracle tất định. Với TUI có `tui-test` (`trace.json`, `.cast`).
- **Bước nhỏ nhất:** trên một Device Linux test, cài `cua-driver`, rồi `rcli exec <dev> -- cua-driver call get_desktop_state …` và đọc JSON trả về. Cú pháp `call` cụ thể chưa kiểm: **UNVERIFIED**.
- **Đánh đổi:**
  - Phải cài thêm phần mềm trên từng Device.
  - `exec` chạy theo Terminal User. Việc nó có vào đúng phiên GUI, D-Bus a11y và TCC grant của user đó hay không: **UNVERIFIED**, đặc biệt với macOS TCC.
  - Không dùng được ở màn khoá. Extension perception của Cua là AGPL. Repo này cũng AGPL-3.0 (ADR 0002), nên license không xung đột, nhưng phải kiểm lại khi phân phối.

### Không khuyến nghị cho repo này
- UI-TARS / UI-TARS-desktop, `macos-automator-mcp`, `appium-windows-driver`/WinAppDriver, SikuliX1: archived hoặc không còn bảo trì.
- Terminator: chỉ Windows, hoạt động chậm từ 2026-06. Scrapybara: SDK đứng từ 2025-05.
- Open Interpreter OS mode: không còn trong sản phẩm chính.
- E2B Desktop, cua sandbox, OSWorld/WAA làm môi trường chạy: dành cho VM/container dùng một lần. rcli đã điều khiển máy thật. Chỉ nên mượn **cách viết task và evaluator** của OSWorld.
- Tosca Vision AI, Eggplant: thương mại, đóng, trùng vai trò với rcli (điều khiển qua VNC/RDP).
- Mobile (Maestro, mobile-mcp, appium-mcp, Midscene Android/iOS): rcli không có Agent cho Android/iOS. Nếu cần test mobile thì dùng song song, Maestro là lựa chọn tất định nhất (có `assertWithAI` tùy chọn).
