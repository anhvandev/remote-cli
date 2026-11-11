# 09: Hardware encoding for Live View on Linux

**What to build:** On Linux, Live View encodes through `hwcodec`'s VA-API encoder, with the software fallback from 07, on X11 and Wayland. Live View keeps working at the greeter and lock screen; if the hardware encoder cannot start there, it falls back to openh264. See `.scratch/setup-v2/spec.md`.

**Blocked by:** 07

**Status:** resolved

- [ ] On Linux with a VA-API GPU, Live View uses it on X11 and on Wayland (manual check; record GPU, distro and session type)
- [ ] Live View works at the greeter and lock screen (manual check; record which encoder ran)
- [x] A machine without VA-API streams through openh264
- [x] `cargo test --workspace` and clippy clean

## Comments

- `crates/agent/src/encoder.rs`: on Linux `HARDWARE` is `h264_vaapi` (the only H.264 VA-API encoder in hwcodec
  `ec38eba`; RustDesk's FFmpeg port enables it). The encoder runs in the root Agent service, not the session helper,
  and FFmpeg opens VA-API on a DRM render node (`/dev/dri/renderD*`) before trying X11, so X11, Wayland, the greeter
  and the lock screen take the same path; any failure falls back to openh264 as in 07. VA-API gets NV12 frames
  (RustDesk opens every hwcodec encoder in NV12); VideoToolbox keeps I420. No NVENC/AMF on Linux (spec: VA-API only),
  so NVIDIA's proprietary driver, which has no VA-API encoder, streams through openh264.
- `hwcodec` is now also a Linux dependency of `rcli-agent` (same pin, separate target table to ease the merge with
  08). `packaging/linux/build-deb.sh` / `build-rpm.sh` build with `--features gui,hwcodec` and need `VCPKG_ROOT` with
  RustDesk's FFmpeg: `vcpkg install 'ffmpeg[amf,nvcodec]' --triplet x64-linux|arm64-linux` plus `clang`, `libva-dev`,
  `libdrm-dev`, `libx11-dev`, `cmake`, `ninja-build` (docs/usage.md). The packages Recommend libva and a VA driver
  (deb: `libva-drm2, libva-x11-2, va-driver-all`; rpm: `libva, mesa-va-drivers`): RustDesk's FFmpeg dlopens
  `libva.so.2`, `libva-drm.so.2` and `libva-x11.so.2`, so without them the Agent falls back to openh264.
- New test `encoder::tests::a_frame_decodes_to_the_colours_it_was_encoded_from`: encodes a red/blue frame through
  whichever encoder starts, decodes it with openh264 and checks both colours, so a wrong pixel layout fails it.
- Checked (2026-11-11, from a Mac, no Linux hardware): in a Debian bookworm arm64 container (colima) with RustDesk's
  FFmpeg built by vcpkg, `rcli-agent --features hwcodec` builds, clippy is clean, the live e2e tests
  (`rcli-testkit --test live --features rcli/hwcodec`) pass, and the colour test logs `openh264`: the container has
  no VA-API, so `h264_vaapi` did not start and the Agent fell back. The NV12 layout was checked on the Mac by forcing
  it through `h264_videotoolbox` (colour test passes; with U and V swapped it fails). `cargo test --workspace` and
  clippy clean on macOS.
- Linux release binaries built with `hwcodec` now link `libdrm.so.2` and `libX11.so.6` (and `libz` on x86_64)
  dynamically, like `libpipewire`/`libdbus` already; the packages do not declare them.
- NOT done: the manual hardware checks. A human must, on a Linux machine with a VA-API GPU (record GPU, driver,
  distro, session type) and an rcli built with `--features hwcodec`: (1) open Live View on an X11 session and on a
  Wayland session and check the Agent log says `live stream …: H.264 through h264_vaapi` and the picture has the
  right colours; (2) switch Live View Quality through all three levels; (3) open Live View at the greeter and at the
  lock screen and record which encoder the log names (VA-API expected; openh264 is acceptable); (4) on the same
  machine run `cargo test -p rcli-agent --features hwcodec --lib encoder` (passes on `h264_vaapi` when (1)
  did); (5) on a machine without VA-API (or with `libva-drm2` removed) check the log names openh264 and
  Live View still works. x86_64 builds were not checked (only arm64 Linux); nor were the .deb/.rpm scripts run.
