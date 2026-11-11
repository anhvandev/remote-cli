# Hướng dẫn deploy

Toàn bộ hệ thống chạy bằng một file Docker Compose trên một VPS: Server, Postgres, Dashboard và coturn (TURN relay cho WebRTC).

> **Giới hạn hiện tại:** Agent mới chỉ có FakePlatform (màn hình và máy giả). Adapter cho Windows, macOS, Linux (ticket 28–34) chưa làm, nên **chưa điều khiển được máy thật**. Phần Server, CLI, Dashboard và MCP đã chạy được đầy đủ.

## 1. Chuẩn bị VPS

- Docker + Docker Compose v2.
- Mở các cổng:

| Cổng | Giao thức | Dùng cho |
|---|---|---|
| 8080 | TCP | Controller API (CLI, MCP, Dashboard gọi vào) |
| 8443 | TCP | Agent kết nối (mTLS) |
| 3080 | TCP | Dashboard |
| 3478 | TCP + UDP | TURN/STUN (coturn) |
| 49160–49200 | UDP | Dải cổng relay của coturn |

- Đặt TLS (reverse proxy như Caddy/nginx) trước cổng 8080 và 3080 nếu mở ra internet. Cổng 8443 đã là TLS sẵn, **không** đặt proxy trước nó. Nếu proxy bật gzip hoặc buffer, tắt cho các đường stream (`/api/v1/devices/*/shell`, `/api/devices/*/shell`), nếu không web terminal sẽ bị đứng.

## 2. Biến môi trường

Tạo file `.env` cạnh `compose.yaml`:

```bash
cat > .env <<EOF
RCLI_BOOTSTRAP_ADMIN_TOKEN=$(openssl rand -hex 16)
RCLI_TURN_SECRET=$(openssl rand -hex 16)
RCLI_MASTER_KEY=$(openssl rand -hex 32)
RCLI_TURN_HOST=vps.example.com
RCLI_AGENT_HOSTNAMES=vps.example.com
POSTGRES_PASSWORD=$(openssl rand -hex 16)
EOF
```

| Biến | Bắt buộc | Ý nghĩa |
|---|---|---|
| `RCLI_BOOTSTRAP_ADMIN_TOKEN` | có | Token admin tạm để tạo admin đầu tiên. Chỉ có hiệu lực khi chưa có admin nào còn phiên đăng nhập hợp lệ. |
| `RCLI_TURN_SECRET` | có | Secret chung giữa Server và coturn để cấp credential TURN. |
| `RCLI_TURN_HOST` | nên đặt | Tên/IP public của VPS mà trình duyệt và Agent dùng để tới coturn. Mặc định `localhost`. |
| `RCLI_AGENT_HOSTNAMES` | chỉ lần đầu | Server Address ban đầu (đưa vào chứng chỉ TLS cổng 8443). Sau lần khởi động đầu, đổi bằng `rcli server-address set` hoặc trang Server Settings. Mặc định `localhost`. |
| `RCLI_MASTER_KEY` | nên đặt | 64 ký tự hex. Mã hoá CA private key và TOTP secret. |
| `RCLI_TRUSTED_PROXIES` | nên đặt khi có proxy | IP của reverse proxy TLS đứng trước cổng 8080, cách nhau bằng dấu phẩy (vd. `172.17.0.1` khi proxy chạy trên host và gọi vào container). Chỉ với request từ các IP này, Server mới lấy IP thật của Controller từ `X-Forwarded-For` (proxy phải thêm header này). Không đặt thì mọi Controller sau proxy dùng chung IP của proxy: nhập sai Device Password vài lần sẽ khoá tất cả, và audit chỉ ghi IP của proxy. |
| `POSTGRES_PASSWORD` | nên đặt | Mặc định `rcli`. |

**Về `RCLI_MASTER_KEY` — đọc kỹ:**
- Không có key: Server vẫn chạy, nhưng CA key và TOTP secret nằm dạng plaintext trong Postgres (Server log cảnh báo).
- Có key: lần khởi động đầu, Server tự mã hoá CA key và TOTP secret đang là plaintext.
- **Sao lưu key.** Sau khi dữ liệu đã mã hoá, chạy Server mà thiếu key thì Server không khởi động được; đổi sang key khác thì dữ liệu đã mã hoá không mở được. Chưa có chức năng xoay key.

## 3. Khởi động

```bash
docker compose up -d --build
docker compose ps        # cả 4 service: postgres, server, dashboard, coturn đều phải "running"
```

Lần build đầu mất vài phút (biên dịch Rust và `next build`).

## 4. Tạo admin đầu tiên

Cần binary `rcli` trên máy của bạn (xem `docs/usage.md`, mục Cài CLI).

```bash
rcli setup --server https://vps.example.com:8080 --key rk_xxxxxxxx_yyyyyyyy   # Server Key (bước 5)
rcli --token "$RCLI_BOOTSTRAP_ADMIN_TOKEN" user create me --role admin
rcli --token "$RCLI_BOOTSTRAP_ADMIN_TOKEN" user set-password me --password '<mật khẩu Dashboard>'
#   → in ra totp_secret và otpauth_url: quét otpauth_url bằng app Authenticator
rcli login me                       # mật khẩu Dashboard + mã TOTP; phiên được lưu vào config
```

Từ lúc admin `me` đăng nhập, bootstrap token hết tác dụng. Nó chỉ hoạt động lại nếu mọi admin bị vô hiệu hoá hoặc phiên đã hết hạn (đường cứu hộ).

Mở `http://vps.example.com:3080`, đăng nhập bằng `me` + mật khẩu + mã TOTP. Sai 5 lần liên tiếp thì tài khoản bị khoá 15 phút.

## 5. Thêm Device (Agent tự đăng ký)

```bash
rcli server-key show                          # đã `rcli login me` ở bước 4
#   rk_f90a7c21b8..._3f9c...                                ← Server Key: fingerprint CA + secret
```

Trên máy cần quản lý:

```bash
rcli agent run \
  --server vps.example.com:8443 \
  --key rk_f90a7c21b8..._3f9c... \
  --state-dir /var/lib/rcli \
  --name ten-may
```

`--socket` (mặc định `/run/rcli/agent.sock`, trên macOS `/var/run/rcli/agent.sock`) là socket IPC nội bộ.
Trên chính Device đó: `rcli agent info` xem Device ID + trạng thái kết nối (mọi user); `sudo rcli agent password show` xem
Device Password (chỉ root). Device Password được sinh lúc đăng ký và lưu mã hoá trong `--state-dir`.

- Agent từ chối kết nối nếu CA của Server không khớp fingerprint trong Server Key (chống giả Server).
- Agent tự đăng ký, nhận chứng chỉ mTLS, **Device ID 9 chữ số** và public key cập nhật (pin lại). Không cần tạo mã enroll.
- Sau lần đầu, credential nằm trong `--state-dir`; các lần chạy sau chỉ cần `--server` và `--key`.
- Cài lại Agent luôn tạo Device mới. Muốn giữ lịch sử: `rcli device merge <cũ> --into <mới>` (dùng Device ID nếu hai Device trùng tên).
- Muốn Agent nhận bản cập nhật tự động (mục 7): đặt `RCLI_UPDATE_PUBLIC_KEY=<public key hex>` cho Server

> Hiện `rcli agent run` dùng FakePlatform: Device hiện Online, có inventory/metrics giả, nhận Action nhưng không điều khiển Device thật.

## 6. Wake qua Wake Relay

Wake chỉ hoạt động khi trong cùng LAN (cùng subnet) với máy cần bật có **một Agent khác đang Online** làm Wake Relay. Nếu không có, `rcli wake` báo `no_wake_relay` kèm subnet. Cách thường làm: để một máy nhỏ luôn bật trong mỗi LAN.

## 7. Cập nhật Agent (bản build có chữ ký)

```bash
rcli rollout keygen                      # một lần: in private key (giữ bí mật, KHÔNG để trên Server) và public key
rcli rollout publish --help              # ký và upload một bản build cho một target OS/arch
rcli rollout <version> --all             # hoặc --device <id> ...
rcli devices                             # cột AGENT / UPDATE cho biết tiến độ
```

Server không giữ private key và không tự kiểm chữ ký; chính Agent kiểm bằng public key đã pin lúc đăng ký (`RCLI_UPDATE_PUBLIC_KEY` của Server). "--all" chỉ gồm các Device đã có lúc rollout.

Mục Tải xuống của trang Connect guide: `rcli.exe` là bản build đã rollout cho `windows-x86_64`; `.dmg`, `.deb` và
`.rpm` là package do Admin upload (build bằng `packaging/macos/build-dmg.sh`, `packaging/linux/build-deb.sh`,
`build-rpm.sh`). Mỗi loại và mỗi kiến trúc giữ một bản, upload lại là thay:

```bash
rcli package publish rcli.dmg --arch aarch64
rcli package publish rcli_0.1.0_amd64.deb --arch x86_64
rcli package publish rcli-0.1.0-1.x86_64.rpm --arch x86_64
```

## 8. Vận hành

- **Dữ liệu:** tất cả nằm trong volume Postgres `pgdata`. Sao lưu bằng `docker compose exec postgres pg_dump -U rcli rcli > backup.sql`.
- **Lưu trữ:** metrics 5 giây giữ 7 ngày, metrics 1 phút giữ 90 ngày, audit giữ 90 ngày; tự dọn, không cần cron.
- **Log:** `docker compose logs -f server`.
- **Dừng:** `docker compose down` (giữ dữ liệu) hoặc `docker compose down -v` (xoá cả Postgres).

## 9. Các điểm chưa kiểm hoặc còn hở

- coturn mới được kiểm là khởi động được và cấp credential đúng; live view qua TURN trên mạng thật (NAT khó) chưa thử.
- Khoá đăng nhập chỉ tính theo user, chưa theo IP.
- `rcli exec` không giới hạn kích thước output; lệnh có timeout dài giữ HTTP request mở tới 1 giờ.
- Không có HA/cluster (ngoài phạm vi spec).

## 10. Cài Agent trên Linux (systemd, greeter, deb/rpm)

Agent chạy như một service root `rcli-agent.service`; service tự tìm phiên đồ hoạ trên seat0 (X11 hoặc Wayland;
màn hình khoá hoặc greeter của GDM/LightDM/SDDM) và chạy helper `rcli agent session` dưới đúng user của phiên đó.
Nhờ vậy Controller xem và điều khiển được cả màn hình khoá lẫn màn hình đăng nhập, không chỉ phiên đã đăng nhập.
Trên Wayland, màn hình đăng nhập của GDM không có portal nên không chụp được — xem mục "Wayland" bên dưới.

**Yêu cầu:** systemd; một phiên đồ hoạ **X11 (Xorg) hoặc Wayland** (GNOME, KDE, wlroots). X11 cần `xauth`; Wayland
cần `xdg-desktop-portal` và một backend ScreenCast (`xdg-desktop-portal-gnome`/`-kde`/`-wlr`) đang chạy trong phiên
người dùng. Nên cài `tesseract-ocr` (OCR); `systemd-creds` có sẵn trong systemd hiện đại, dùng TPM nếu máy có.

### 10.1. Cài trực tiếp (không cần package)

```bash
# Trên máy Linux, với binary rcli đã build/tải về:
sudo install -m 755 ./rcli /usr/local/bin/rcli
sudo rcli agent install --server vps.example.com:8443 --key rk_<fingerprint>_<secret>   # thêm --allow-root nếu muốn
```

Kết quả mong đợi:

```
installed rcli-agent.service and /usr/local/lib/rcli/rcli
Device ID: 123456789
Device Password: abcdEFGH12
```

- Sai Server Key (sai secret hoặc sai fingerprint CA) → lệnh **thất bại ngay**, không tạo Device, không cài service.
- Terminal User mặc định là `SUDO_USER` (người chạy `sudo`), ghi vào `/var/lib/rcli/settings.json`. Đổi bằng
  `sudo rcli agent terminal-user set <user>` sau khi service chạy.
- `--allow-root` đặt Device Setting `root=always`; mặc định là `ask`.
- Server Key nằm trong `/etc/rcli/agent.env` (mode `0600`, chỉ root đọc), không nằm trên command line của service.

Kiểm tra ngay:

```bash
systemctl status rcli-agent.service --no-pager   # active (running)
rcli agent info                                  # Device ID + "connected"
sudo journalctl -u rcli-agent.service -n 30 --no-pager
```

### 10.2. Reboot, greeter, lock screen, unlock

```bash
sudo reboot
# sau khi máy lên:
systemctl is-enabled rcli-agent.service && systemctl is-active rcli-agent.service   # enabled, active
rcli agent info                                                                     # connected
```

Các bước kiểm tra bằng mắt (chạy từ máy Controller):

1. Đăng nhập desktop, `rcli shot <Device ID> --out /tmp/shot` → thấy màn hình desktop.
2. Khoá màn hình (`loginctl lock-session` tại máy đó, hoặc `rcli power <Device ID> lock`), rồi `rcli shot` → thấy
   màn hình khoá. `rcli act <Device ID> key enter` và `rcli unlock <Device ID>` (sau `sudo rcli agent unlock-password set`
   tại máy đó) → mở khoá được.
3. Đăng xuất ra greeter (GDM/LightDM/SDDM), `rcli shot` → thấy màn hình đăng nhập; `rcli act` gõ được vào ô mật khẩu;
   `rcli unlock` gõ mật khẩu + Enter để đăng nhập.
4. `rcli exec <Device ID> -- whoami` → đúng Terminal User; `rcli shell <Device ID>` mở được shell.
5. `rcli metrics <Device ID> history` vẫn có dữ liệu (daemon tự lấy metrics, không phụ thuộc helper).

Chạy contract suite trên chính máy đó (cần root; X11 cần một X server, Wayland cần phiên có portal):

```bash
cargo build --release -p rcli-agent --bin rcli-contract
sudo ./target/release/rcli-contract --session-socket /run/rcli/session.sock
# làm theo hướng dẫn trên terminal (focus text field, lock, unlock, logout, login, unlock)
# → bảng markdown: mọi ô ở cột "Logged in", "Lock screen", "Login screen" phải ✅
```

Không có `--session-socket` thì suite chạy trực tiếp trên phiên của người gọi (dùng để kiểm tra riêng phiên
đăng nhập, ví dụ `rcli-contract --state-dir ~/.local/state/rcli-contract` trên Wayland để giữ token portal). Trên
Wayland, cột "Login screen" sẽ ❌ vì greeter không có portal — đó là giới hạn đã biết, ghi lại kết quả vào ticket.
Làm lần lượt: chạy suite ở phiên đăng nhập, ở màn hình khoá, rồi ở greeter.

### 10.3. Wayland

Phiên người dùng Wayland (GNOME, KDE, Sway/Hyprland) được điều khiển, không cần chuyển sang Xorg. Chụp màn hình đi
qua XDG desktop portal (`org.freedesktop.portal.ScreenCast`) + PipeWire; input do service root ghi vào
`/dev/uinput` (con trỏ tuyệt đối + bàn phím ảo).

- **Phê duyệt một lần:** lần đầu Agent chụp màn hình, một hộp thoại chia sẻ màn hình hiện ở máy đó. Chọn màn hình
  (có thể chọn nhiều) rồi bấm **Share**. Portal trả về một *restore token*, Agent lưu ở
  `/var/lib/rcli/screencast.restore_token` (0600, chỉ root đọc), nên các lần kết nối sau và sau khi khởi động lại
  không hỏi lại. Token được truyền cho helper qua biến môi trường `RCLI_SCREENCAST_RESTORE_TOKEN`.
- **Không có portal:** `rcli shot` trả lỗi rõ ràng ("screen capture on Wayland needs the XDG desktop portal's
  ScreenCast"). Cài `xdg-desktop-portal` + backend (`xdg-desktop-portal-gnome`, `-kde`, hoặc `-wlr` cho
  Sway/Hyprland) rồi `systemctl --user restart xdg-desktop-portal` (và service `rcli-agent.service`).
- **Không có `/dev/uinput`:** input báo lỗi rõ ràng; nạp module một lần: `sudo modprobe uinput` và thêm
  `uinput` vào `/etc/modules-load.d/uinput.conf` để có lại sau reboot.
- **Nhiều màn hình:** chọn nhiều màn hình trong hộp thoại portal; mỗi màn hình là một stream. Chỉ các màn hình đã
  chọn mới chụp được (`rcli displays` liệt kê đúng chúng).
- **Bàn phím:** gõ text theo layout US (mã phím evdev, không phụ thuộc XKB); layout khác chỉ đúng với các phím đơn
  nếu compositor đang dùng layout đó. Phím tên (`enter`, `ctrl`, `f5`, …) dùng mã evdev.
- **Màn hình khoá:** vẫn là phiên Wayland đang chạy, nên chụp/điều khiển qua cùng stream. Nếu compositor dừng
  stream khi khoá, `rcli shot` báo "the screen cast stream sent no frame" — ghi lại hành vi của từng compositor.
- **Greeter Wayland (GDM):** không có portal, nên `rcli shot` báo "screen capture at a Wayland login screen is not
  available". Muốn điều khiển màn hình đăng nhập, đặt `WaylandEnable=false` trong `/etc/gdm3/custom.conf` và
  `sudo systemctl restart gdm` như trước (khi đó greeter chạy Xorg và dùng đường X11).

### 10.4. Deb/RPM

Build trên Linux (cần `dpkg-dev` hoặc `rpm-build`):

```bash
packaging/linux/build-deb.sh     # → packaging/linux/out/rcli_0.1.0_amd64.deb
packaging/linux/build-rpm.sh     # → packaging/linux/out/rcli-0.1.0-1.x86_64.rpm
```

Cài package (package chỉ đặt `/usr/bin/rcli`; hook `postinst` chạy đúng một lệnh `rcli agent install`):

```bash
sudo install -m 600 /dev/null /etc/rcli/agent.conf
sudo tee /etc/rcli/agent.conf >/dev/null <<'CONF'
RCLI_SERVER=vps.example.com:8443
RCLI_SERVER_KEY=rk_<fingerprint>_<secret>
# tuỳ chọn:
# RCLI_TERMINAL_USER=alice
# RCLI_ALLOW_ROOT=1
CONF
sudo apt install ./rcli_0.1.0_amd64.deb      # hoặc: sudo dnf install ./rcli-0.1.0-1.x86_64.rpm
systemctl status rcli-agent.service --no-pager
```

Nếu chưa có `/etc/rcli/agent.conf`, package vẫn cài nhưng in hướng dẫn; sau đó chạy tay:

```bash
sudo rcli agent install --server "$(sed -n 's/^RCLI_SERVER=//p' /etc/rcli/agent.conf)" \
     --key "$(sed -n 's/^RCLI_SERVER_KEY=//p' /etc/rcli/agent.conf)"
```

Gỡ package: hook `prerm` gọi `rcli agent uninstall` trước, nên Device bị xoá khỏi Server và dữ liệu cục bộ biến mất:

```bash
sudo apt remove rcli        # hoặc sudo dnf remove rcli
sudo apt purge rcli         # Debian: xoá luôn /etc/rcli/agent.conf
```

### 10.5. Nâng cấp

- Có package mới: `sudo apt install ./rcli_<version>_<arch>.deb` → `postinst` chép binary mới vào
  `/usr/local/lib/rcli/rcli` và `systemctl restart rcli-agent.service` (service chạy binary mới, Device ID giữ nguyên).
- Không dùng package: `sudo rcli upgrade` thay binary đang chạy và service (build do Server rollout, có chữ ký).
- Sau khi nâng cấp: `systemctl is-active rcli-agent.service` = active, `rcli agent info` = connected.

### 10.6. Headless server

Cùng một lệnh `sudo rcli agent install --server … --key …`; không có DISPLAY cũng không sao:

- service vẫn chạy và tự kết nối lại sau reboot;
- `rcli metrics`, `rcli exec`, `rcli shell`, `rcli power` hoạt động;
- `rcli shot`, `rcli act`, Live View trả lỗi "no X11 session on the console" (Device không có màn hình);
- `systemctl status rcli-agent.service` và `journalctl -u rcli-agent.service` là nơi xem log.
