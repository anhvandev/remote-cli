# Hướng dẫn sử dụng

Có ba cách điều khiển, cùng gọi một Controller API nên cùng quyền, cùng Control Lock, cùng audit:

- **`rcli`**: CLI, công cụ chính. Mọi lệnh có `--json` cho script và AI.
- **`rcli mcp`**: cùng các thao tác đó dưới dạng MCP tools cho Claude Code và các AI agent khác.
- **Dashboard** (trình duyệt): xem fleet, biểu đồ, live view, điều khiển trực tiếp, web terminal, nút power/Wake, màn hình admin.

Từ ngữ (Device, Action, Observation, Mark, Control Lock, Wake Relay…) theo `GLOSSARY.md`. Cách dựng Server xem `docs/deploy.md`.

> Agent hiện chạy FakePlatform: mọi lệnh bên dưới chạy được, nhưng màn hình, input và shell là giả (shell chỉ hiểu `echo`, `whoami`, `sleep`).

## Cài CLI

```bash
cargo build --release -p rcli
# binary: target/release/rcli (Controller và Agent trong cùng một binary)
rcli setup --server http://localhost:8080 --key rk_xxxxxxxx_yyyyyyyy   # một lần
rcli connect 123456789                                                  # hỏi Device Password một lần
```

`rcli setup` lưu Server Address, Server Key và update key đã pin vào thư mục cấu hình của bạn (thiếu `--server` hoặc
`--key` thì lệnh hỏi, Server Key nhập không hiện ra màn hình). Server Address phải là
`https://` (TLS ở reverse proxy); `http://` chỉ được nhận khi trỏ tới chính máy này (localhost, cho test hoặc Server
chạy cùng máy) — không có cờ nào cho phép http tới máy khác. `rcli connect` kiểm tra Device ID + Device Password rồi
nhớ mật khẩu (OS keychain: Keychain trên macOS, Credential Manager trên Windows, Secret Service trên Linux; file
`0600` khi không có keychain hoặc khi dùng `--config-dir`), nên các lệnh sau chỉ cần Device ID.
`rcli forget <Device ID>` xoá mật khẩu đã nhớ. `RCLI_SERVER`/`RCLI_TOKEN` không còn dùng.

Lệnh quản trị Server (`user`, `rollout`, `audit`, `server-key`, `server-address`, đổi tên/xoá Device, `lock force-release`)
cần một phiên User: `rcli login <tên>` (mật khẩu Dashboard + mã TOTP), `rcli logout` để xoá.
Operator đăng nhập được nhưng các lệnh đó trả `admin_required`. Các lệnh trên Device vẫn dùng `rcli connect`.

## Cài trên Windows

Agent chạy như một Windows service tên `rcli-agent` (tài khoản LocalSystem, tự khởi động cùng máy). Ba cách cài, cùng một kết quả:

```powershell
# 1. Tải rcli.exe, bấm đúp: cửa sổ rcli mở với ô Server Address + Server Key và nút Install (UAC hỏi một lần).
# 2. Một dòng trong PowerShell bình thường (script tự xin quyền qua UAC):
iwr https://<server>/install.ps1 -OutFile install.ps1; .\install.ps1 -Key <Server Key>
# 3. Trong PowerShell "Run as administrator", với binary đã tải:
rcli agent install --server <server>:<port> --key <Server Key>
```

Cài xong in ra Device ID và Device Password. Binary ở `%ProgramFiles%\rcli\rcli.exe`; dữ liệu Device (credentials, Device Settings, Secret Store) ở `%ProgramData%\rcli`, chỉ SYSTEM và Administrators đọc được. Server Key nằm trong `%ProgramData%\rcli\service.json`, **không** nằm trên command line của service (`sc qc rcli-agent` không thấy nó). Secret Store dùng DPAPI machine scope (`CryptProtectData` + `CRYPTPROTECT_LOCAL_MACHINE`), nên copy ổ đĩa hay bản backup sang máy khác không đọc được Device Password.

Nếu Server Key sai, lệnh cài thất bại ngay và không để lại service hay file nào.

**Màn khoá, màn đăng nhập và UAC.** Service chạy trong session 0 nên không thấy desktop của người dùng. Nó chạy một helper (`rcli agent session`) trong phiên console bằng token SYSTEM của chính nó, chuyển vào phiên đó (`DuplicateTokenEx` + `SetTokenInformation(TokenSessionId)`, cần `SeTcbPrivilege`) — đúng token mà winlogon dùng, nên mở được cả desktop `Winlogon` (màn khoá, màn đăng nhập) và secure desktop của UAC (`WTSQueryUserToken` chỉ là phương án dự phòng). Helper chuyển sang input desktop trước mỗi lần chụp/gõ. Vì vậy `shot` và `act` chạy được ở màn khoá, màn đăng nhập và hộp thoại UAC. Helper được chạy lại khi phiên console hoặc người đăng nhập đổi (đăng nhập, đăng xuất, chuyển user). Không cần cấp quyền Screen Recording như macOS. Lưu ý: vì helper chạy dưới SYSTEM, OCR (Windows.Media.Ocr) dùng profile của SYSTEM; nếu `--text` không nhận được chữ ở màn khoá, cài thêm gói ngôn ngữ OCR cho máy.

**Ctrl+Alt+Del.** Windows chỉ cho tiến trình SYSTEM gửi secure attention sequence qua `SendSAS` (sas.dll), và phải bật policy một lần trên mỗi Device (mặc định là tắt):

```powershell
# PowerShell as administrator, một lần:
Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' `
  -Name SoftwareSASGeneration -Type DWord -Value 1
# 0 = tắt (mặc định), 1 = cho phép ứng dụng chạy dưới quyền SYSTEM (đủ cho rcli), 2 = cho phép mọi ứng dụng
```

Rồi từ Controller: `rcli lock acquire <Device ID>` và `rcli act <Device ID> key ctrl+alt+del`. Không bật policy thì Windows bỏ qua `SendSAS` và không có gì xảy ra.

**Gỡ cài đặt** (PowerShell "Run as administrator"):

```powershell
rcli agent uninstall
```

Lệnh này gỡ Device khỏi Server (nếu Server không tới được thì chỉ cảnh báo, phần cài đặt cục bộ vẫn được xoá), dừng và xoá service, xoá `%ProgramFiles%\rcli` và `%ProgramData%\rcli` (kể cả Secret Store) — giống macOS/Linux. Log của service ở `%ProgramData%\rcli\agent.log`.

Build chưa ký nên SmartScreen có thể cảnh báo khi chạy `rcli.exe` vừa tải: chọn "More info" → "Run anyway" (trang Connect của Dashboard giải thích).

## Vai trò

Role chỉ còn quản trị Server; việc gì chạy được trên Device do Device Settings của chính Device đó quyết định.

| | operator | admin |
|---|---|---|
| Xem fleet, metrics, lịch sử, màn hình | ✅ | ✅ |
| Điều khiển (khi giữ Control Lock), `exec`, `shell`, `--as root` | ✅ | ✅ |
| Restart/Shut down/Sleep khi người khác giữ lock | ❌ | ❌ |
| Users, Server Key, Server Address, Device (đổi tên/xoá/thu hồi/gộp), rollout Agent, audit, force-release lock | ❌ | ✅ |

## Xem fleet

```bash
rcli devices                         # danh sách + Device Status (Online / Offline / Waking / Offline (Wake failed))
rcli device show demo-pc             # inventory + tài nguyên hiện tại + màn khoá / màn đăng nhập
rcli device processes demo-pc        # danh sách process lấy ngay lúc gọi
rcli displays demo-pc                # các màn hình: độ phân giải, scale, vị trí
rcli metrics history demo-pc --from 2026-11-04T00:00:00Z --resolution 1m
```

Device nhận cả id lẫn tên. Nếu hai Device trùng tên, dùng id.

## Nhìn và điều khiển màn hình

**Nhìn** không cần lock:

```bash
rcli shot demo-pc --out ./shots                 # ảnh màn hình (JPEG, cạnh dài 1280)
rcli shot demo-pc --display all --out ./shots   # một ảnh cho mỗi màn hình
rcli shot demo-pc --marks                       # đánh số Marks lên ảnh + in bảng Mark
```

Ảnh không đổi so với lần trước thì nhận `unchanged` thay vì gửi lại.

**Điều khiển** cần giữ Control Lock:

```bash
rcli lock acquire demo-pc
rcli act demo-pc click 640 360 --out ./shots          # tọa độ tính trong ảnh Observation, Agent tự đổi sang pixel thật
rcli act demo-pc click --text "OK"                    # click theo chữ trên màn hình
rcli act demo-pc click --mark 3 --observation <id>    # click theo số Mark của lần --marks trước
rcli act demo-pc type "xin chao" --observe none       # không cần ảnh trả về
rcli act demo-pc type "matkhau" --secret              # che chữ trong audit log
rcli act demo-pc key ctrl+shift+t
rcli act demo-pc scroll 640 360 --dy 3
rcli lock release demo-pc
```

Chế độ Observation (`--observe`):

| Mode | Trả về | Dùng khi |
|---|---|---|
| `shot` (mặc định) | 1 ảnh, chờ màn hình đứng yên tối đa ~1 s | hầu hết trường hợp |
| `none` | không ảnh | gõ liên tục, không cần nhìn |
| `frames` | chỉ các frame có thay đổi rõ, có mốc thời gian (tối đa 10; đổi qua field `max_frames` của API, 1–30) | theo dõi animation |
| `clip` | MP4 H.264 tối đa 5 s → `clip.mp4` (chỉ `rcli act`, không có ở `rcli batch`) | người xem lại thao tác |

**Action Batch**: nhiều Action một lượt, một Observation:

```bash
echo '[{"type":"key","keys":"ctrl+a"},{"type":"wait","ms":200},{"type":"type","text":"hi"}]' \
  | rcli batch demo-pc - --observe frames --out ./shots
```

Bước nào lỗi thì batch dừng và báo `step` (đếm từ 0). Tổng `wait` tối đa 5000 ms.

**Lỗi hay gặp:**

- `control_lock_held`: người khác đang giữ lock, lỗi kèm tên và giờ hết hạn. Chờ rồi thử lại, không có hàng đợi.
- `stale_mark`: số Mark từ Observation cũ, chụp lại với `--marks`.
- `ambiguous_text` / `text_not_found`: chữ khớp nhiều chỗ hoặc không khớp chỗ nào.

**Control Lock** tự hết sau 10 phút không có Action (mọi request của người giữ, kể cả chỉ chụp, đều gia hạn). `rcli mcp` và Dashboard còn được nhả sau 60 s nếu mất kết nối; CLI thường thì không. Admin: `rcli lock force-release demo-pc` (có ghi audit).

## Màn khoá và màn đăng nhập

rcli không lưu mật khẩu đăng nhập OS ở đâu cả (ADR 0005). Controller tự gõ mật khẩu bằng `type` có `--secret`:

```bash
rcli lock acquire demo-pc
rcli act demo-pc type '...' --secret --observe none  # che chữ trong audit log
rcli act demo-pc key enter --out ./shots
```

`rcli device show` cho biết `session`: `Active`, `LockScreen` hoặc `LoginScreen`. Chữ gõ khi Device đang ở màn khoá
hoặc màn đăng nhập luôn bị che trong audit, kể cả khi quên `--secret`. Trên Windows, xem thêm mục **Cài trên Windows**
để bật policy cho `Ctrl+Alt+Del`. Màn đăng nhập macOS chỉ xem được, nên Mac cần bật auto-login nếu phải chịu được reboot.

## Terminal

```bash
rcli exec demo-pc -- echo hello                     # in stdout/stderr, thoát với exit code của lệnh (124 nếu hết giờ)
rcli exec demo-pc --timeout 300 -- ./long-job.sh
rcli exec demo-pc --as root -- whoami               # chỉ admin
rcli shell demo-pc                                  # shell tương tác như SSH; --as root cho admin
```

Terminal không cần Control Lock. Lệnh chạy dưới quyền user đang đăng nhập; nếu chưa ai đăng nhập thì chỉ admin chạy được (dưới root/SYSTEM).

## Power và Wake

```bash
rcli power demo-pc lock         # khoá màn hình, không cần lock
rcli power demo-pc restart      # restart / shutdown / sleep: bị từ chối nếu người khác giữ Control Lock (trừ admin)
rcli wake demo-pc               # Device Offline → Waking; quá 3 phút chưa Online thì "Wake failed"
```

## Admin

```bash
rcli login an                                 # hỏi mật khẩu Dashboard + mã TOTP, lưu phiên vào config
rcli logout                                   # thu hồi và xoá phiên đã lưu
rcli user create an --role operator
rcli user set-password an --password '...'   # in TOTP secret + otpauth URL cho Dashboard
rcli user disable an
rcli server-key show                          # Server Key (rk_<CA fingerprint>_<secret>) để cài Agent
rcli server-key rotate                        # đổi secret: Device đã đăng ký vẫn Online, bản cài mới cần key mới
rcli server-address show                      # Server Address Agent dùng
rcli server-address set vps.example.com       # cấp lại chứng chỉ TLS cho tên mới
rcli device rename demo-pc may-ke-toan
rcli device revoke <id>                       # Agent bị cắt và không kết nối lại được
rcli device merge <id-cũ> --into <id-mới>     # gộp lịch sử sau khi cài lại máy
rcli device delete <id>
rcli audit --device demo-pc --since 2026-11-01T00:00:00Z
```

Audit giữ 90 ngày: ai, lúc nào, Device nào, tham số, kết quả, exit code của lệnh terminal. Mật khẩu, chữ gõ với `secret` và chữ gõ ở màn khoá/màn đăng nhập bị che; ảnh, clip và phím gõ trong `shell` không bao giờ được lưu.

## Dashboard

Mở `http://<server>:3080`, đăng nhập bằng tên + mật khẩu + mã TOTP.

- **Fleet**: danh sách Device với Device Status.
- **Trang Device**: inventory, tài nguyên hiện tại, màn khoá/màn đăng nhập, người giữ Control Lock và giờ hết hạn, biểu đồ lịch sử (1h / 24h / 7d / 90d), nút Wake / Lock screen / Sleep / Restart / Shut down.
- **Live view**: xem màn hình trực tiếp qua WebRTC, chọn màn hình nếu Device có nhiều. Khi bạn giữ Control Lock thì click và gõ trực tiếp lên video. Người khác giữ lock thì vẫn xem được và thấy ai giữ đến lúc nào.
- **Web terminal**: ngay dưới live view; admin có thêm ô "as root".
- **Quản trị** (chỉ admin): users, rollout Agent, đổi tên/gộp/thu hồi/xoá Device.

## Dùng với Claude Code (MCP)

```bash
rcli setup --server http://localhost:8080 --key rk_xxxxxxxx_yyyyyyyy
rcli connect 123456789
claude mcp add rcli -- /đường/dẫn/rcli mcp
```

`rcli mcp` dùng cùng cấu hình và Device Password đã nhớ, nên không cần biến môi trường.

Tools: `devices`, `device`, `lock_acquire`, `lock_release`, `lock_status`, `observe` (shot / frames / clip / marks), `act`, `batch`, `exec`, `wake`, `power`. Ảnh trả về inline, bảng Mark nằm trong kết quả có cấu trúc. Mỗi AI agent nên dùng một user Dashboard riêng (qua `rcli login`) để audit tách bạch.

Ở màn khoá hoặc màn đăng nhập, AI agent tự gõ mật khẩu OS mà người dùng đưa cho nó bằng `type` có `"secret": true`; rcli không lưu mật khẩu này. Chưa có tool `shell` trong MCP; dùng `exec`.

## Thử nhanh trên một máy

```bash
# 1. Dựng stack theo docs/deploy.md với RCLI_TURN_HOST=localhost, rồi tạo admin `me`.
# 2. Enroll một Agent giả:
rcli server-key show                              # Server Key (rk_<CA fingerprint>_<secret>)
rcli agent run --server localhost:8443 --key <server-key> \
  --state-dir /tmp/agent-state --name demo-pc
# 3. Ở terminal khác:
rcli devices && rcli shot demo-pc && rcli exec demo-pc -- echo hello
```
