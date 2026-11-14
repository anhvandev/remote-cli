# Hướng dẫn sử dụng

Có ba cách điều khiển, cùng gọi một Controller API nên cùng quyền, cùng Control Lock, cùng audit:

- **`rcli`**: CLI, công cụ chính. Mọi lệnh có `--json` cho script và AI.
- **`rcli mcp`**: cùng các thao tác đó dưới dạng MCP tools cho Claude Code và các AI agent khác.
- **Dashboard** (trình duyệt): xem fleet, biểu đồ, live view, điều khiển trực tiếp, web terminal, nút power/Wake, màn hình quản lý.

Từ ngữ (Device, Action, Observation, Mark, Control Lock, Wake Relay…) theo `GLOSSARY.md`. Cách dựng Server xem `docs/deploy.md`.

> Agent hiện chạy FakePlatform: mọi lệnh bên dưới chạy được, nhưng màn hình, input và shell là giả (shell chỉ hiểu `echo`, `whoami`, `sleep`).

## Cài rcli

Mỗi máy chỉ cài một lần, bằng một lệnh: máy đó thành một Device và điều khiển được các Device khác ngay (ADR 0006).
Trang **Hướng dẫn kết nối** của Dashboard hiện sẵn các lệnh cài với Server Address và Server Key điền sẵn.

> **Build trên macOS**: build dev và `cargo test` không cần gì thêm, Live View mã hoá bằng openh264. Bản release
> (`packaging/macos/build-dmg.sh`) bật feature `hwcodec`: Live View mã hoá H.264 bằng VideoToolbox qua `hwcodec` của
> RustDesk, cần FFmpeg tĩnh của RustDesk build bằng vcpkg, một lần cho mỗi máy build (cần `pkg-config`, `nasm` và
> Python ≥ 3.10 trong `PATH`): `git clone https://github.com/microsoft/vcpkg ~/vcpkg && ~/vcpkg/bootstrap-vcpkg.sh`,
> lấy thư mục `res/vcpkg` của repo `rustdesk/rustdesk`, rồi
> `~/vcpkg/vcpkg install ffmpeg --triplet arm64-osx --overlay-ports=<rustdesk>/res/vcpkg` (`x64-osx` trên Mac Intel).
> Sau đó build bằng `VCPKG_ROOT=~/vcpkg cargo build --release -p rcli --features hwcodec`. Không có GPU encoder (máy
> ảo) hoặc encoder không khởi động được thì Agent tự chuyển sang openh264; log Agent ghi tên encoder đang dùng
> (`live stream …: H.264 through h264_videotoolbox` hoặc `… through openh264`).
>
> **Build trên Linux**: tương tự, `packaging/linux/build-deb.sh` / `build-rpm.sh` bật `hwcodec` để Live View mã hoá bằng
> VA-API (`h264_vaapi`). Máy build cần thêm `clang`, `libva-dev`, `libdrm-dev`, `libx11-dev`, `cmake`, `ninja-build`,
> rồi `~/vcpkg/vcpkg install 'ffmpeg[amf,nvcodec]' --triplet x64-linux --overlay-ports=<rustdesk>/res/vcpkg`
> (`arm64-linux` trên ARM; trên ARM đặt thêm `VCPKG_FORCE_SYSTEM_BINARIES=1`). FFmpeg nạp libva lúc chạy: máy không có
> libva, driver VA-API hoặc GPU thì Agent dùng openh264, kể cả ở greeter và màn hình khoá.

```bash
cargo build --release -p rcli
# binary: target/release/rcli (Controller và Agent trong cùng một binary)
sudo rcli install --server vps.example.com:8443 --key rk_xxxxxxxx_yyyyyyyy   # một lần cho mỗi máy
rcli connect 123456789                                                        # hỏi Device Password một lần
```

`rcli install` (root/Administrator) cài Agent, đăng ký Device, in Device ID và Device Password, rồi ghi cấu hình
Controller dùng chung cho cả máy (`/etc/rcli/controller.json`, trên Windows `%ProgramData%\rcli\controller.json`):
Server Address, Server Key và API Address. Mọi user trên máy đọc được file này nhưng không sửa được,
nên ai cũng chạy được `rcli connect` và `rcli mcp` mà không cài lại. Server Address là `host:port` của cổng Agent
(8443). Không cần nhập URL nào khác: Server gửi API Address (URL của Dashboard) lúc đăng ký và mỗi lần Agent kết nối.
Server tự ghi lại API Address lần đầu một User mở Dashboard qua `https://`. Trước đó cài vẫn thành công, nhưng các
lệnh Controller báo `the Server has no API Address yet`. Server Key sai thì lệnh cài thất bại ngay và không để lại gì.

API Address phải là `https://`; `http://` chỉ được nhận khi trỏ tới chính máy này (localhost, cho test hoặc Server
chạy cùng máy) — không có cờ nào cho phép http tới máy khác. Chạy thử trên một máy, Dashboard ở `http://localhost:3080`:
User đặt API Address ở Server Settings của Dashboard, hoặc
`rcli --server http://localhost:3080 api-address set http://localhost:3080` (sau `rcli --server http://localhost:3080 login <tên>`).

`rcli uninstall` (root/Administrator) gỡ Device khỏi Server, gỡ Agent và cấu hình dùng chung; Device Password đã nhớ
của từng user được giữ lại cho lần cài sau. `rcli connect` kiểm tra Device ID + Device Password rồi
nhớ mật khẩu (OS keychain: Keychain trên macOS, Credential Manager trên Windows, Secret Service trên Linux; file
`0600` khi không có keychain hoặc khi dùng `--config-dir`), nên các lệnh sau chỉ cần Device ID.
`rcli forget <Device ID>` xoá mật khẩu đã nhớ. `RCLI_SERVER`/`RCLI_TOKEN` không còn dùng.

Lệnh quản trị Server (`user`, `audit`, `server-key`, `server-address`, `api-address`, `build`, đổi tên/xoá Device, `lock force-release`)
cần một phiên User: `rcli login <tên>` (mật khẩu Dashboard + mã TOTP), `rcli logout` để xoá.
Mọi User đều chạy được các lệnh này (không có vai trò). Kết nối bằng Device Password thì không: chúng trả
`user_session_required`. Các lệnh trên Device vẫn dùng `rcli connect`.

## Cài trên Windows

Agent chạy như một Windows service tên `rcli-agent` (tài khoản LocalSystem, tự khởi động cùng máy). Ba cách cài, cùng một kết quả:

```powershell
# 1. Tải rcli.exe, bấm đúp: cửa sổ rcli mở với ô Server Address + Server Key và nút Install (UAC hỏi một lần).
# 2. Một dòng trong PowerShell bình thường (script tự xin quyền qua UAC):
iwr https://<server>/install.ps1 -OutFile install.ps1; .\install.ps1 -Key <Server Key>
# 3. Trong PowerShell "Run as administrator", với binary đã tải:
rcli install --server <server>:8443 --key <Server Key>
```

Cài xong in ra Device ID và Device Password. Binary ở `%ProgramFiles%\rcli\rcli.exe`; dữ liệu Device (credentials, Device Settings, Secret Store) ở `%ProgramData%\rcli`, chỉ SYSTEM và Administrators đọc được. Server Key nằm trong `%ProgramData%\rcli\service.json`, **không** nằm trên command line của service (`sc qc rcli-agent` không thấy nó). Secret Store dùng DPAPI machine scope (`CryptProtectData` + `CRYPTPROTECT_LOCAL_MACHINE`), nên copy ổ đĩa hay bản backup sang máy khác không đọc được Device Password.

Nếu Server Key sai, lệnh cài thất bại ngay và không để lại service hay file nào.

**Màn khoá, màn đăng nhập và UAC.** Service chạy trong session 0 nên không thấy desktop của người dùng. Nó chạy một helper (`rcli agent session`) trong phiên console bằng token SYSTEM của chính nó, chuyển vào phiên đó (`DuplicateTokenEx` + `SetTokenInformation(TokenSessionId)`, cần `SeTcbPrivilege`) — đúng token mà winlogon dùng, nên mở được cả desktop `Winlogon` (màn khoá, màn đăng nhập) và secure desktop của UAC (`WTSQueryUserToken` chỉ là phương án dự phòng). Helper chuyển sang input desktop trước mỗi lần chụp/gõ. Vì vậy `shot` và `act` chạy được ở màn khoá, màn đăng nhập và hộp thoại UAC. Helper được chạy lại khi phiên console hoặc người đăng nhập đổi (đăng nhập, đăng xuất, chuyển user). Không cần cấp quyền Screen Recording như macOS. Lưu ý: vì helper chạy dưới SYSTEM, OCR (Windows.Media.Ocr) dùng profile của SYSTEM; nếu `--text` không nhận được chữ ở màn khoá, cài thêm gói ngôn ngữ OCR cho máy.

**Live View và GPU.** Bản `rcli.exe` release build trên máy Windows với feature `hwcodec`: Live View mã hoá H.264 bằng
NVENC (NVIDIA), AMF (AMD) hoặc QSV (Intel) qua `hwcodec` của RustDesk. Cần FFmpeg tĩnh của RustDesk (một lần cho mỗi máy
build, như mục Build trên macOS): `vcpkg install ffmpeg[amf,nvcodec,qsv] --triplet x64-windows-static --overlay-ports=<rustdesk>\res\vcpkg`,
rồi trong PowerShell `$env:VCPKG_ROOT='C:\vcpkg'; $env:RUSTFLAGS='-C target-feature=+crt-static'; cargo build --release -p rcli --features gui,hwcodec`
(CRT tĩnh vì `hwcodec` và triplet `x64-windows-static` dùng `/MT`). FFmpeg của RustDesk không có Media Foundation
(`h264_mf`). Service (session 0) mã hoá, helper chỉ chụp, nên màn khoá, màn đăng nhập và UAC dùng cùng encoder; máy
không có GPU encoder (máy ảo) hoặc encoder không khởi động được thì Agent tự chuyển sang openh264, log Agent ghi tên
encoder (`live stream …: H.264 through h264_nvenc` hoặc `… through openh264`).

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
rcli uninstall
```

Lệnh này gỡ Device khỏi Server (nếu Server không tới được thì chỉ cảnh báo, phần cài đặt cục bộ vẫn được xoá), dừng và xoá service, xoá `%ProgramFiles%\rcli` và `%ProgramData%\rcli` (kể cả Secret Store) — giống macOS/Linux. Log của service ở `%ProgramData%\rcli\agent.log`.

Build chưa ký nên SmartScreen có thể cảnh báo khi chạy `rcli.exe` vừa tải: chọn "More info" → "Run anyway" (trang Connect của Dashboard giải thích).

## Không có vai trò

Mọi User làm được mọi việc: điều khiển Device và quản lý Server (Users, Server Key, Server Address, Device, Agent
Build, audit, force-release lock). Việc gì chạy được trên Device do Device Settings của chính Device đó quyết định.
Không User nào tự vô hiệu chính mình hay vô hiệu User cuối cùng còn hoạt động.

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

Mỗi Mark có `number`, `text` (nhãn), `role`, `display` và khung. Container (`window`, `dialog`, `menu`, `list`) cũng là Mark: có số, được vẽ lên ảnh và nhắm được theo số hoặc chữ. Thêm ba field, bỏ trống thì không có trong JSON:

- `states`: các Mark State khác mặc định, trong `disabled`, `focused`, `checked`, `selected`, `expanded`, `collapsed`.
- `value`: nội dung (chữ trong ô, giá trị slider), tách khỏi nhãn `text`. Ô mật khẩu không bao giờ có `value`. Nhắm theo chữ chỉ khớp `text`.
- `parent`: số của Mark container gần nhất chứa nó, để phân biệt hai nút "OK" ở hai dialog.

Bảng Mark của CLI có thêm cột `PARENT`, `STATES`, `VALUE` (value dài bị cắt, `--json` có đủ). Mark từ OCR (màn khoá, không có cây accessibility) không có ba field này.

Trên macOS, Marks lấy từ cửa sổ đang focus cùng menu bar, menu đang mở và sheet của app đó; cửa sổ khác của app nằm phía sau không còn cho Mark. Thay đổi hành vi: ô nhập mà nhãn duy nhất là nội dung của nó (`AXValue`) giờ có `text` rỗng, nội dung nằm ở `value`, nên nhắm ô đó theo nội dung không còn khớp; nhắm theo số Mark thay thế.

**Wait Condition**: chờ tới khi một chữ hiện ra hoặc biến mất rồi mới chụp, thay vì đoán thời gian chờ:

```bash
rcli shot demo-pc --until-appears "Saved" --marks          # chờ chữ "Saved" hiện ra
rcli shot demo-pc --until-disappears "Loading" --timeout-ms 20000
rcli shot demo-pc --until-appears "OK" --role button       # chỉ tính Mark có role button
```

API: thêm `"until": {"appears": {"text": "Saved", "role": "button"}, "timeout_ms": 5000}` (hoặc `"disappears"`) vào
request `actions`, chỉ với observe `shot`. Agent đọc lại Marks khoảng mỗi 100 ms (OCR khi không có accessibility tree,
nên dùng được ở màn khoá và màn đăng nhập) và trả Observation khi hai lần đọc liên tiếp đều thoả. Chữ khớp như khi
target bằng chữ: không phân biệt hoa thường, bằng hoặc chứa. `timeout_ms` mặc định 5000, tối đa 30000. Hết giờ thì lỗi
`wait_timeout`, và trong object `error` vẫn có `images` (cùng `observation`, `marks` nếu xin Marks) của lần đọc cuối.
Chờ không cần Control Lock.

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

Bước nào lỗi thì batch dừng và báo `step` (đếm từ 0). Tổng thời gian chờ của một request (các `wait` cộng `timeout_ms` của Wait Condition) tối đa 30000 ms.

**Lỗi hay gặp:**

- `control_lock_held`: người khác đang giữ lock, lỗi kèm tên và giờ hết hạn. Chờ rồi thử lại, không có hàng đợi.
- `stale_mark`: số Mark từ Observation cũ, chụp lại với `--marks`.
- `ambiguous_text` / `text_not_found`: chữ khớp nhiều chỗ hoặc không khớp chỗ nào.
- `wait_timeout`: Wait Condition không thoả trong `timeout_ms`; lỗi kèm Observation của lần đọc cuối.

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

## Quản lý Server

```bash
rcli login an                                 # hỏi mật khẩu Dashboard + mã TOTP, lưu phiên vào config
rcli logout                                   # thu hồi và xoá phiên đã lưu
rcli user create an
rcli user set-password an --password '...'   # in TOTP secret + otpauth URL cho Dashboard
rcli user disable an
rcli server-key show                          # Server Key (rk_<CA fingerprint>_<secret>) để cài Agent
rcli server-key rotate                        # đổi secret: Device đã đăng ký vẫn Online, bản cài mới cần key mới
rcli server-address show                      # Server Address Agent dùng
rcli server-address set vps.example.com       # cấp lại chứng chỉ TLS cho tên mới
rcli api-address show                         # API Address (rỗng tới khi một User mở Dashboard qua https://)
rcli api-address set https://rcli.example.com # sửa API Address; Agent nhận ở lần kết nối sau
rcli device rename demo-pc may-ke-toan
rcli device revoke <id>                       # Agent bị cắt và không kết nối lại được
rcli device merge <id-cũ> --into <id-mới>     # gộp lịch sử sau khi cài lại máy
rcli device delete <id>
rcli build list                               # Agent Build theo phiên bản, mới nhất trước; đánh dấu Latest Build
rcli build upload ./rcli --version 0.3.0 --target linux-x86_64   # Server tự tính sha256
rcli build delete 0.3.0 linux-x86_64          # bỏ target thì xoá cả phiên bản
rcli audit --device demo-pc --since 2026-11-01T00:00:00Z
```

Cập nhật một máy lên Latest Build của OS/arch của nó (trên Device đã cài thì chạy bằng sudo/Administrator để
Agent được restart). Lệnh in phiên bản đang chạy, Latest Build, tiến độ tải, kiểm tra sha256 và restart; không bao giờ
hạ phiên bản. `rcli upgrade` vẫn chạy như alias ẩn.

```bash
sudo rcli update
```

Đặt Minimum Version trong Server Settings: Device dưới mức này hiện "update needed", Controller dưới mức này in cảnh
báo, và Agent dưới mức này tự cập nhật lên Latest Build.

Audit giữ 90 ngày: ai, lúc nào, Device nào, tham số, kết quả, exit code của lệnh terminal. Mật khẩu, chữ gõ với `secret` và chữ gõ ở màn khoá/màn đăng nhập bị che; ảnh, clip và phím gõ trong `shell` không bao giờ được lưu.

## Dashboard

Mở `http://<server>:3080`, đăng nhập bằng tên + mật khẩu + mã TOTP.

- **Fleet**: danh sách Device với Device Status.
- **Trang Device**: inventory, tài nguyên hiện tại, màn khoá/màn đăng nhập, người giữ Control Lock và giờ hết hạn, biểu đồ lịch sử (1h / 24h / 7d / 90d), nút Wake / Lock screen / Sleep / Restart / Shut down.
- **Live view**: xem màn hình trực tiếp qua WebRTC, chọn màn hình nếu Device có nhiều. Video ở độ phân giải gốc của màn hình; chọn Live View Quality trên thanh công cụ (Quality: nét nhất, Balanced: mặc định, Speed: mượt nhất), đổi giữa chừng không phải kết nối lại. Khi bạn giữ Control Lock thì click và gõ trực tiếp lên video. Người khác giữ lock thì vẫn xem được và thấy ai giữ đến lúc nào.
- **Web terminal**: ngay dưới live view; admin có thêm ô "as root".
- **Quản lý** (mọi User): Users, Updates (upload/xoá Agent Build), Settings (Server Key, Server Address, Minimum Version).
- **Thao tác Device**: menu `⋯` trên mỗi dòng ở trang Devices: đổi tên, đổi Device ID, đổi Device Password, gộp, thu hồi, xoá.

## Dùng với Claude Code (MCP)

```bash
rcli connect 123456789                        # trên máy đã `rcli install`
claude mcp add rcli -- /đường/dẫn/rcli mcp
```

`rcli mcp` dùng cấu hình của bản cài và Device Password đã nhớ, nên không cần biến môi trường hay bước thiết lập nào khác.

Tools: `devices`, `device`, `lock_acquire`, `lock_release`, `lock_status`, `observe` (shot / frames / clip / marks / Wait Condition `until`), `act`, `batch`, `exec`, `wake`, `power`. Ảnh trả về inline, bảng Mark nằm trong kết quả có cấu trúc. Mỗi AI agent nên dùng một user Dashboard riêng (qua `rcli login`) để audit tách bạch.

Ở màn khoá hoặc màn đăng nhập, AI agent tự gõ mật khẩu OS mà người dùng đưa cho nó bằng `type` có `"secret": true`; rcli không lưu mật khẩu này. Chưa có tool `shell` trong MCP; dùng `exec`.

## Thử nhanh trên một máy

```bash
# 1. Dựng stack theo docs/deploy.md với RCLI_TURN_HOST=localhost, tạo admin `me`, đặt API Address
#    http://localhost:3080 (mục Cài rcli), rồi cài máy này:
sudo rcli install --server localhost:8443 --key <Server Key>
# 2. Enroll một Agent giả:
rcli server-key show                              # Server Key (rk_<CA fingerprint>_<secret>)
rcli agent run --server localhost:8443 --key <server-key> \
  --state-dir /tmp/agent-state --name demo-pc
# 3. Ở terminal khác:
rcli devices && rcli shot demo-pc && rcli exec demo-pc -- echo hello
```
