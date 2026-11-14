# Báo cáo: danh sách nâng cấp rcli cho test E2E, việc đã lên kế hoạch và việc để lại

Ngày: 2026-11-14. Nhánh `agent-perception`.

**Nguồn:**
- Danh sách và bằng chứng: `docs/research/rcli-improvements.md`.
- Cách các công cụ khác làm, đọc từ mã nguồn: `docs/research/computer-use-source-verification.md`.

**Quyết định nền** (chốt khi grilling): rcli giúp Controller AI **nhìn, chờ và thao tác chính xác hơn**. Kịch bản, runner và kết luận pass/fail là việc của Controller, không nằm trong rcli.

**Bằng chứng:** "code" là đã đọc trong mã nguồn; "README" là lời tự mô tả của tác giả công cụ.

---

## 1. Danh sách nâng cấp, xếp theo giá trị cho E2E

Thứ tự theo nguyên tắc: kiểm tất định trước → chờ đúng lúc → hành động đáng tin → để lại bằng chứng → bộ khung test → mở rộng phạm vi.

| Hạng | Mục | Nâng cấp | Cỡ | Bằng chứng | Trạng thái |
|---|---|---|---|---|---|
| 0 | Bug | `exec`/`shell` trên Windows luôn chạy bằng SYSTEM | M | code | ✅ Đã lên kế hoạch, làm trước |
| 1 | A1 | Mark có Mark State, value, container và `parent` | M | code | ✅ Đã lên kế hoạch (spec 1) |
| 2 | A2 | Wait Condition (`appears`, `disappears`, `value`, `state`) | M | code | ✅ Đã lên kế hoạch (spec 1) |
| 3 | A8 | Windows đọc popup/menu/dialog, thêm giới hạn node và timeout; macOS đọc cửa sổ đang focus | S | code | ✅ Đã lên kế hoạch (spec 1) |
| 4 | A3 | Thao tác semantic (Invoke/AXPress/DoAction) kèm kết quả `effect` | L | code | ⏳ Để lại, chờ spec 2 |
| 5 | A7 | Tham chiếu phần tử ổn định qua nhiều Observation | M | code | ⏳ Để lại, chờ spec 2 |
| 6 | A4 | Ghi lại run: artifact từng bước và trajectory, phía `rcli mcp` | S–M | code | ⏳ Để lại, chờ spec 2 |
| 7 | A5 | `exec` trong phiên GUI của user | M | code | 🟡 Windows nằm trong ticket bug; macOS/Linux chờ spec 2 |
| 8 | A6 | Liệt kê và focus cửa sổ, mở app | M | code | ⏳ Để lại, chờ spec 2 |
| 9 | A9 | Video cả kịch bản (hiện clip tối đa 5 s) | M | code | ⏳ Để lại, chờ spec 2 |
| 10 | A10 | Runner kịch bản kèm verdict | M–L | README (+code UFO) | ❌ Không làm trong rcli |
| 11 | A11 | Report JUnit / `summary.md` | S | README | ❌ Không làm trong rcli |
| 12 | A12 | Chụp màn hình macOS bằng ScreenCaptureKit | S–M | code | 💤 Gác lại |
| 13 | A13 | Terminal/TUI qua MCP | M | README | 💤 Gác lại |
| 14 | A14 | Zoom vào một vùng màn hình | S | README | 💤 Gác lại |
| 15 | A15 | Mobile, dùng Device làm máy cắm điện thoại | L | README | 💤 Gác lại |

---

## 2. Việc đã lên kế hoạch

### 2a. Ticket bug Windows, làm trước
- **Ticket:** `.scratch/windows-terminal-user/issues/01-run-windows-terminal-as-terminal-user.md`, theo ADR 0010.
- **Kết quả:**
  - Lệnh không phải root chạy bằng token của phiên Terminal User đang đăng nhập (console hoặc RDP).
  - User không đăng nhập thì báo `terminal_user_not_signed_in`.
  - `--as root` vẫn chạy bằng SYSTEM và theo Device Settings.
  - `as_root` và audit báo đúng.
  - Giải quyết luôn A5 cho Windows.
- **Thay đổi hành vi:** `exec` không có `--as root` ở màn đăng nhập Windows sẽ bị từ chối.

### 2b. Spec 1: A1 + A2 + A8

**Spec:** `.scratch/agent-perception/spec.md`.

| # | Ticket | Bị chặn bởi |
|---|---|---|
| 01 | Mark có Mark State, value và container, chạy trọn đường trên FakePlatform | — |
| 02 | Windows: state, value, cha, popup/menu/dialog, giới hạn node và timeout | 01 |
| 03 | macOS: state, value, cha, cửa sổ đang focus cùng menu/sheet | 01 |
| 04 | Linux: state, value, cha | 01 |
| 05 | Chờ text xuất hiện/biến mất trong `observe` (không cần Control Lock) | — |
| 06 | Wait Condition làm một bước trong Action Batch | 05 |
| 07 | Chờ theo value và Mark State | 01, 05 |
| 08 | Server chặn Wait Condition gửi tới Agent cũ (`agent_too_old`) | 05 |

**Các quyết định chính:**
- Chỉ ghi Mark State khác mặc định.
- Ô mật khẩu không bao giờ có value.
- Thêm 4 role container: `window`, `dialog`, `menu`, `list`.
- Wait Condition chọn phần tử bằng `{text, role?}`.
- Kiểm mỗi 100 ms, cần 2 lần đọc giống nhau liên tiếp.
- Timeout mặc định 5 s, tối đa 30 s.
- Hết giờ báo `wait_timeout` kèm Observation.

**Kiểm thử:**
- Testkit: mở rộng `tests/marks.rs`.
- Checklist trên máy Windows, macOS và Linux thật cho các ticket 02–05.

**Thay đổi hành vi:**
- Trên macOS, ô mà nhãn duy nhất là giá trị của nó giờ có `text` rỗng.
- Trên Windows, số Mark có thể dịch đi vì có thêm popup và dialog.

---

## 3. Việc để lại

### 3a. Chờ spec 2, chỉ bàn sau khi spec 1 đã ra bản

| Mục | Vì sao chờ | Phải trả lời trước |
|---|---|---|
| A3 thao tác semantic + `effect` | Cần A1 (state/value để đọc lại) và A7 (tham chiếu ổn định) | Helper chạy SYSTEM có gọi được UIA pattern vào app của user không (probe trên máy thật) |
| A7 tham chiếu ổn định | Chỉ cần khi có A3. Wait Condition dùng text + role là đủ | Định danh lại phần tử bằng đường dẫn con + role + pid, hay chấm điểm như Peekaboo |
| A4 ghi lại run | Độc lập, làm phía Controller. Giá trị rõ hơn khi đã có A2 và A3 | Lưu ở đâu, bật thế nào, giữ chính sách "audit không lưu ảnh" |
| A5 cho macOS/Linux | Windows đã nằm trong ticket bug | macOS: tool chạy qua helper Aqua có quyền TCC không. Linux: `env_reset` của sudo |
| A6 cửa sổ và app | Cần A5 để mở app bằng token của user | — |
| A9 video cả kịch bản | Cần A4 | Ghép frame hay nối clip |

### 3b. Không làm trong rcli
- **A10 runner/verdict, A11 report:** đã chốt hướng (a): judging là việc của Controller. Chưa có số đo nào về độ chính xác của LLM chấm trên desktop, và các benchmark nghiêm túc đều chấm bằng script. Nếu sau này cần, làm thành công cụ riêng đứng trên `rcli mcp`.

### 3c. Gác lại

| Mục | Lý do |
|---|---|
| A12 ScreenCaptureKit | Giá trị thấp cho E2E. Chưa rõ có chạy ở màn đăng nhập macOS không |
| A13 Terminal/TUI qua MCP | Bằng chứng mới là README. Tạm dùng `tui-test` qua `exec` (sau A5) |
| A14 zoom vùng | Bằng chứng mới là README, giá trị chưa rõ |
| A15 mobile | Nằm ngoài phạm vi sản phẩm hiện tại |

### 3d. Không nên làm
- LLM tự chấm bên trong Agent hoặc Server.
- Input nền bằng API private của macOS.
- Hook `WH_*_LL`, telemetry.
- Lấy OmniParser làm perception mặc định (weights AGPL).
- Phụ thuộc vào các công cụ đã ngừng: UI-TARS, Shortest, Magnitude, Octomind, SikuliX.

---

## 4. Thứ tự tiếp theo
1. Ticket bug Windows. Làm song song với ticket 01 và 05 của spec 1.
2. Ticket 02, 03, 04 (theo OS) sau 01. Ticket 06, 07, 08 sau 05.
3. Chạy cả bộ test (`cargo test --workspace`, Dashboard e2e) một lần sau khi merge đủ ticket. Chạy checklist trên máy thật.
4. Ra bản, dùng thử, rồi mới grilling spec 2 (A3, A7, A4, A5 cho macOS/Linux, A6, A9).
