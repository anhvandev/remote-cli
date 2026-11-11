# 08: Hardware encoding for Live View on Windows

**What to build:** On Windows, Live View encodes through `hwcodec`'s Windows encoders (Media Foundation, NVENC, QSV or AMF, whichever the machine has), with the software fallback from 07. Live View keeps working at the lock screen, the login screen and the UAC prompt, where the Agent's SYSTEM helper captures; if the hardware encoder cannot start there, it falls back to openh264. See `.scratch/setup-v2/spec.md`.

**Blocked by:** 07

**Status:** ready-for-agent

- [ ] On Windows with a GPU, Live View uses a hardware encoder (manual check; record GPU and Windows version)
- [ ] Live View works at the lock screen, login screen and UAC prompt (manual check; record which encoder ran)
- [ ] A Windows VM without a GPU encoder streams through openh264
- [ ] `cargo test --workspace` and clippy clean

## Comments

- `encoder.rs`: the `hwcodec` path now builds on Windows too; `HARDWARE` there is `h264_nvenc`, `h264_amf`,
  `h264_qsv`, `h264_mf`, tried in order, openh264 when none starts (07's fallback, unchanged). Names checked against
  hwcodec `ec38eba` (`src/ffmpeg_ram/encode.rs`, `cpp/common/util.cpp`) and RustDesk's FFmpeg port, which enables
  `h264_nvenc`, `h264_amf`, `h264_qsv` on Windows x64 but not `h264_mf` (`--disable-everything`, no
  `--enable-mediafoundation`): with that FFmpeg, Media Foundation never starts and just logs "did not start".
- Hardware frames are now NV12 on every OS (were I420): QSV takes only NV12 (hwcodec drops QSV for I420), and NVENC
  goes through a D3D11 frame pool, which has no I420. New unit test `encoder::tests::encoded_frames_decode_back_to_their_colours`
  encodes four colour quadrants through whichever encoder `H264::open` picks and decodes them with openh264. Run with
  `--features hwcodec` on this Mac it exercised `h264_videotoolbox` with NV12 and passed; swapping U and V made it fail
  (`quadrant 0 came back [50, 84, 250]`), so it catches a wrong layout. On Windows, `cargo test -p rcli-agent
  --features hwcodec encoder` runs it through the machine's GPU encoder.
- Lock screen, login screen, UAC: the SYSTEM helper only captures; frames go over the pipe to the service (session 0),
  which runs Live View and the encoder. So those screens use the same encoder as the desktop, and if a hardware
  encoder cannot start in session 0, `H264::open` falls back to openh264 there too. No helper-side code was needed.
- Build: `hwcodec` is now also a Windows-only optional dependency (same rev). Release build on Windows:
  `vcpkg install ffmpeg[amf,nvcodec,qsv] --triplet x64-windows-static --overlay-ports=<rustdesk>\res\vcpkg`, then
  `VCPKG_ROOT=… RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p rcli --features gui,hwcodec`
  (docs/usage.md, Windows section). The repo has no Windows packaging script to update.
- Checked on this Mac: `cargo xwin check -p rcli-agent --target x86_64-pc-windows-msvc` (without hwcodec) is clean.
  With hwcodec it cannot be cross-checked: hwcodec's `build.rs` picks the Windows include path and SDK by
  `#[cfg(windows)]` of the build host, so it only builds on Windows. The Rust in `mod hardware` is the same on both OSes
  and compiles under `--features hwcodec` on macOS.
- **NOT done: no manual check on Windows hardware.** A human must, on a Windows build with `--features gui,hwcodec`:
  1. Confirm it builds and links (static CRT with `+crt-static`; FFmpeg port features `amf,nvcodec,qsv`).
  2. Run `cargo test -p rcli-agent --features hwcodec encoder` on a GPU machine: passes, and the GPU encoder is the one
     used (temporarily print `encoder.name()` or check the Agent log).
  3. On an NVIDIA, AMD and/or Intel machine, open Live View: the Agent log says `H.264 through h264_nvenc` / `h264_amf` /
     `h264_qsv`; record GPU, driver and Windows version, and try all three Live View Quality levels.
  4. With the Agent installed as the service, lock the machine, sign out to the login screen and trigger a UAC prompt
     while watching Live View: the picture keeps coming; record which encoder the log names there (session 0 may refuse
     the GPU, in which case it must say openh264 and still stream).
  5. On a VM without a GPU encoder, Live View streams and the log says `through openh264`.
  6. Laptops with two GPUs (Intel + NVIDIA): NVENC is tried first; check it starts and the picture is right.
