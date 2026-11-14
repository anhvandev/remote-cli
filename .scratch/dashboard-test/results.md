# Kết quả test Dashboard remote.ava.pw — 2026-11-14 22:50–23:26

Device: `180577493` (VanNguyen, Ubuntu 24.04, X11, Chrome đăng nhập `admin`).
Phạm vi: chỉ đọc. Người điều khiển: `admin` qua rcli.
Ảnh chụp: `/tmp/rcli-test-ava/`.

| TC | Nội dung | Kết quả |
|---|---|---|
| 1 | Session + điều hướng | **PASS** |
| 2 | Trang Device, lọc tên + trạng thái, xoá lọc | **PASS** |
| 3 | Chi tiết Device — Tổng quan (Online + Offline) | **PASS** |
| 4 | Tab Màn hình — Live view + Control Lock | **PASS** |
| 5a | Tab Terminal — mở, chạy lệnh | **PASS** (có lỗi gõ ký tự) |
| 5b | Tab Lịch sử — biểu đồ, chọn khoảng | **PASS** (lỗi layout) |
| 6 | Hướng dẫn kết nối | **PASS** |
| 7 | Quản lý: /admin, users, updates, settings | **PASS** |
| 8 | /signin + redirect khi chưa đăng nhập | **PASS** |

## Lỗi phát hiện

### B1 — Agent 0.4.1: session helper treo, mất toàn bộ capture/input (nghiêm trọng)
Lúc 22:58 giữa phiên test, `rcli shot` trả `no_display`, `rcli displays` trả `[]`,
Agent log `no GUI session: the rcli session agent is not running` và trả về đúng
request mình gửi thay vì reply.

- Helper PID 12588 vẫn chạy 2h32m, còn kết nối tới `/run/rcli/session.sock`, treo ở `unix_stream_data_wait`.
- Reconciler (`console()` trong `crates/agent/src/linux/session.rs:607`) thấy child còn sống nên **không spawn lại** → treo vĩnh viễn.
- X server vẫn khoẻ (`xwd -root` đọc được, monitor DP-2 1920x1080).
- Khôi phục: `kill 12588` → reconciler spawn helper mới sau 2 s, capture trở lại ngay.

**Đề xuất:** reconciler cần health-check helper (probe `State` có timeout, hoặc kill khi `helper_count()==0` quá N giây), không chỉ `child_is_running`.

### B2 — X11 giữ ký tự khi gõ nhanh (trung bình)
`rcli act … type` làm rơi ký tự: `https://remote.ava.pw/admin/users` → `remte.ava.pw/admin/users`,
`…/devices/<uuid>/history` → `…/devices/04bdf33e-6c8b-420d-ad72-cbf58dbeecd2/history`.
Log: `WARN enigo::platform::x11: fast text entry is not possible on X11`.
Cùng một lệnh typed vào Terminal của Dashboard sinh thêm ký tự `-` thừa
(`whoami` → `-whoami`). Ảnh hưởng cả Controller lẫn terminal trong Dashboard.

### B3 —Trang Lịch sử tràn layout ở 1280px (nhẹ)
Nhãn các mount disk (`/var/lib/docker/overlay2/…`, `/mnt/9C59C3434281BA2C`, `/mnt/3FE05D4FBE23CC74`)
chồng lên nhau và tràn ra ngoài khung biểu đồ bên phải. Cần ellipsis hoặc rút gọn
`overlay2/<hash>` thành `overlay2/…`.

### B4 — Trang chi tiết Device tràn ngang, card Phần cứng bị cắt (trung bình)
Xem lại lúc 23:30–23:39, ảnh `device-overflow-100pct.jpg` và `device-overflow-80pct.jpg`.

**Hiện tượng:** ở 1280×720 (cả 100% và 80% zoom), card **Phần cứng** bị cắt ở mép phải:
giá trị OS/CPU/GPU/Disk/Serial hiện nửa chừng (`Ubunt…`, `Van…`, `/var/lib/docker/overlay2/336d…/me…`),
không có thanh cuộn ngang nên phần bị cắt **không đọc được**. Card **Tài nguyên** cũng bị đẩy rộng ra,
các ô `Disk /var/lib/docker/overlay2/…` (nhãn 96 ký tự) chồng nhau và tràn khỏi lưới.

**Nguyên nhân:**
- `dashboard/app/(app)/devices/[id]/page.tsx:42` — lưới `lg:grid-cols-[minmax(0,1fr)_22rem]`: cột phải cố định 22rem.
- `page.tsx:150` — ô `<td>` có `break-words` (= `overflow-wrap: break-word`), nhưng thuộc tính này
  **không làm giảm min-content width** của table auto-layout. Table vẫn lấy từ dài nhất không có chỗ ngắt
  (đường dẫn mount 96 ký tự ≈ 500 px) làm bề rộng tối thiểu → section rộng hơn track 22rem → tràn ra ngoài viewport.
- `page.tsx:140` — nhãn `Disk {mount}` trong `Kpi` không có utility ngắt dòng nào, cùng một đường dẫn 96 ký tự.
- Nguồn dữ liệu: Device này có 7 mount Docker overlay (`/var/lib/docker/overlay2/<hash>/merged`), và
  chúng xuất hiện **hai lần** — trong `resources.disk_used_percent` (card Tài nguyên) và `inventory.disks`
  (card Phần cứng).

**Cách sửa tối thiểu:** dùng `wrap-anywhere` (Tailwind 4, `overflow-wrap: anywhere` — có làm giảm min-content
width, khác `break-words`) cho `<td>` và cho nhãn `Kpi`; thêm `min-w-0` cho hai `<section>` trong lưới.
Trang Lịch sử (B3) cùng gốc: nhãn mount dài trong legend biểu đồ.

**Chưa sửa** — chỉ ghi nhận.

## Ghi chú vận hành

- Lúc test, tôi bấm nhầm nút **Restart** trên trang Device. Hộp xác nhận hiện ra và tôi đã bấm **Cancel** —
  máy không restart.
- Root `--as root` bị từ chối vì Device Settings đặt root = `ask` và không ai bấm đồng ý tại máy.
- Control Lock đã release lúc 23:26.
