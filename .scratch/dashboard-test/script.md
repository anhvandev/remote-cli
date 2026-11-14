# Kịch bản test Dashboard remote.ava.pw (chỉ đọc)

Thiết bị: `180577493` (VanNguyen, Ubuntu, Chrome đã đăng nhập sẵn user `admin`).
Phạm vi: chỉ đọc — không bấm nút thay đổi dữ liệu (Thay Server Key, Lưu, Vô hiệu, Xoá, + User, Upload).
Công cụ: `rcli` (shot/act/exec), Control Lock giữ suốt phiên.

## TC1 — Phiên đăng nhập và điều hướng

1. Mở `https://remote.ava.pw/` → thấy sidebar Remote CLI + mục Device, Hướng dẫn kết nối, Quản lý.
2. Kiểm tra không bị đẩy về `/signin` (phiên còn hiệu lực).

Kỳ vọng: trang Device hiện ra, sidebar có `admin`, `Đăng xuất`.

## TC2 — Trang Device (danh sách)

1. Heading "Device" kèm 3 số: Online / Offline / Waking.
2. Bảng có cột Device, Trạng thái, Agent, Lần cuối thấy.
3. Mỗi dòng hiện tên + Device ID 9 số.
4. Lọc theo tên: gõ `Van` vào ô "Tìm Device" → chỉ còn các Device khớp.
5. Lọc trạng thái: chọn `Offline` → chỉ còn Device Offline (480882456 thời điểm test).
6. Xoá lọc → về đủ danh sách.

Kỳ vọng: số lượng khớp với `rcli devices --json`; URL đổi `?q=` / `?status=`, Back/refresh giữ lọc.

## TC3 — Chi tiết Device: Tổng quan

1. Bấm `VanNguyen 180577493` → `/devices/<uuid>`.
2. Tab Tổng quan: card Tài nguyên (CPU, GPU, RAM, Disk đọc/ghi, Mạng ↑/↓, Disk % từng mount, Uptime) và card Phần cứng (OS, Hostname, CPU, RAM, Disk, Mạng, Màn hình, Serial).
3. Tab hiện tại được đánh dấu (nền mực, `aria-current=page`).

Kỳ vọng: CPU/RAM có số (Device Online); OS khớp `rcli device show` (Ubuntu, hostname VanNguyen).

## TC4 — Tab Màn hình

1. Bấm tab Màn hình → Live View kết nối, hiện ảnh màn hình thật của Agent.
2. Card Control Lock hiện người giữ (hoặc "Không ai giữ").
3. Ảnh trong Live View khớp ảnh `rcli shot` chụp cùng lúc (khác độ trễ vài giây).

Kỳ vọng: có video, không có lỗi đỏ.

## TC5 — Tab Terminal và Lịch sử

1. Tab Terminal → ô nhập lệnh hiện ra; **không chạy lệnh ghi**, chỉ chạy `whoami` nếu an toàn.
2. Tab Lịch sử → biểu đồ CPU/RAM theo thời gian, chọn khoảng thời gian đổi được.

Kỳ vọng: trang tải < 5 s, không lỗi; số liệu lịch sử không trống với Device Online.

## TC6 — Hướng dẫn kết nối

1. Mở `/connect-guide`.
2. Có Server Address, Server Key (che/không che), lệnh `curl … install.sh`, lệnh PowerShell, mục Tải xuống.
3. Nếu thấy cảnh báo HTTP → ghi nhận (Dashboard đang chạy qua https nên không được có).

Kỳ vọng: Server Address = `remote.ava.pw:9443`; cảnh báo HTTP không hiển thị.

## TC7 — Quản lý (chỉ đọc)

1. `/admin` → 3 mục User / Cập nhật Agent / Server Settings kèm số liệu.
2. `/admin/users` → bảng user, badge "đã vô hiệu" nếu có. Không bấm "+ User", "Đặt mật khẩu", "Vô hiệu".
3. `/admin/updates` → danh sách version + target, nhãn Latest, kích thước, sha256, người upload. Không upload/xoá.
4. `/admin/settings` → 4 card: Server Key, Server Address, API Address, Minimum Version. Không bấm nút Lưu/Thay.

Kỳ vọng: không lỗi 500; API Address là `https://remote.ava.pw` (ghi nhận được khi User mở qua https).

## TC8 — Sign-in (không đăng xuất)

Không đăng xuất phiên admin đang dùng (cần mật khẩu + TOTP). Chỉ kiểm tra `/signin` trả về form Tên đăng nhập / Mật khẩu / Mã TOTP nếu mở tab mới — bỏ qua nếu phải đăng xuất.

## Ghi chú

- Mọi bước chụp ảnh lưu ở `/tmp/rcli-test-ava/`.
- Nếu `rcli` cần Control Lock mà người khác đang giữ → dừng, báo người dùng.
- Kết quả PASS/FAIL ghi kèm ảnh tương ứng.
