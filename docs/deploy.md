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
