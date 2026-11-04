# PROTOTYPE: lockscreen-probe (code dùng một lần, không đưa vào main)

**Câu hỏi cần trả lời (Q48):** Agent chạy bằng quyền root có **chụp được màn hình** và **gửi được phím** ở các màn sau không?

1. macOS: màn đăng nhập (login window)
2. macOS: màn khoá *(thêm, vì chưa có bằng chứng)*
3. Wayland: màn khoá
4. Wayland: màn đăng nhập (greeter)

Mỗi vòng, probe làm 3 việc: gõ phím `a`, chờ 1 giây, rồi chụp màn hình. Nếu cả hai cùng chạy được thì ảnh sẽ có **một dấu chấm trong ô mật khẩu**.

## macOS (`macos/`)

Probe thử hai cách chụp: CGDisplayStream (cách `scrap` của RustDesk đang dùng) và ScreenCaptureKit.

```sh
./install.sh              # build và cài thành LaunchAgent, chỉ chạy ở màn đăng nhập (LoginWindow)
```

> ⚠️ Probe gõ phím `a` vào ô mật khẩu. Trước khi nhập mật khẩu thật, **xoá hết các dấu chấm trong ô**.

- **Màn khoá:** Trong Terminal, chạy `sleep 5; /usr/local/libexec/lockscreen-probe /tmp/lockscreen-probe 6`, rồi khoá máy bằng ⌃⌘Q trong vòng 5 giây. Chờ khoảng 1 phút rồi mở khoá.
- **Màn đăng nhập:** Chọn  → Log Out. Chờ ở màn đăng nhập khoảng 1 phút rồi đăng nhập lại.
- Xem kết quả trong `/tmp/lockscreen-probe/`: file `probe.log` và các ảnh `*-ds.jpg` / `*-sck.jpg`. Ảnh có `uid0` là chụp ở màn đăng nhập.
- Thử xong thì chạy `./uninstall.sh` để gỡ.

## Linux Wayland (`linux/`)

Cách chụp là đọc thẳng màn hình qua DRM/KMS bằng `ffmpeg kmsgrab`, không phụ thuộc compositor. Phím được gửi qua uinput.

```sh
sudo apt install ffmpeg python3-evdev        # Fedora: dnf install ffmpeg python3-evdev
sudo systemd-run --unit=lockscreen-probe "$PWD/probe.sh" 30   # chạy khoảng 5 phút, vẫn chạy tiếp khi log out
```

- Trong 5 phút đó: khoá màn hình, chờ khoảng 30 giây, mở khoá; sau đó log out, chờ khoảng 30 giây ở greeter rồi đăng nhập lại.
- Xem kết quả trong `/var/tmp/lockscreen-probe/`: file `probe.log` cho biết ở mỗi vòng session đang khoá hay đang ở greeter, và các ảnh `N-kms.jpg`.
- Nếu máy có nhiều GPU thì chọn card khác bằng `CARD=/dev/dri/card1`.
- Nếu `kms: FAILED` ở **mọi** vòng, kể cả khi đã đăng nhập bình thường, thì đó là lỗi môi trường (thường là driver NVIDIA proprietary hoặc thiếu vaapi), không phải câu trả lời cho câu hỏi.

## Kết luận

- **macOS (đã chốt, Q49 = a):** ở màn đăng nhập chỉ xem được, không điều khiển được. Màn khoá điều khiển đầy đủ. Xem `docs/adr/0001`.
- **Linux Wayland:** chưa thử, đang chờ có máy.

## Kết quả (điền sau khi thử)

| Môi trường | Chụp ở màn khoá | Phím ở màn khoá | Chụp ở greeter/màn đăng nhập | Phím ở greeter/màn đăng nhập | Ghi chú |
|---|---|---|---|---|---|
| macOS 15.8.1 (CGDisplayStream) | ✅ | ✅ (thấy dấu chấm) | ✅ khi probe đã được cấp Screen Recording | ❌ không được: sau 7 vòng không có dấu chấm nào. `combined→hid` (cách enigo dùng) và `combined→session` báo posted nhưng không có tác dụng; `hidSystem→hid` treo. Đã thử cả `ProcessType=Interactive` và xoá cờ modifier | Lần 3 (2026-11-04): ở màn đăng nhập, quyền đã cấp trong System Settings có tác dụng với root. Khi chưa cấp quyền, SCK báo lỗi -3801. Ở màn khoá có hiện chỉ báo "Your screen is being observed". |
| macOS 15.8.1 (ScreenCaptureKit) | ✅ | (dùng chung phím ở trên) | ✅ khi đã cấp quyền | (dùng chung phím ở trên) | |
| GNOME Wayland | | | | | |
| KDE Wayland | | | | | |
| wlroots (Sway/Hyprland) | | | | | |
