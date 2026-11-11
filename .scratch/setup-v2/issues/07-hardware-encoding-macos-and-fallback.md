# 07: Hardware encoding for Live View on macOS, with software fallback

**What to build:** Live View encodes H.264 through RustDesk's `hwcodec` (ADR 0002), starting with VideoToolbox on macOS. The three Live View Quality levels drive the hardware encoder's bitrate and frame rate. When no hardware encoder exists or it fails to start, the Agent silently falls back to openh264, and the Agent log names the encoder in use. No HW/SW indicator in the Dashboard. See `.scratch/setup-v2/spec.md`.

**Blocked by:** 06

**Status:** ready-for-agent

- [x] On a Mac, Live View uses VideoToolbox (manual check; record the machine and macOS version)
- [x] With the hardware encoder forced unavailable, Live View still works through openh264 (test)
- [x] All three Live View Quality levels work on the hardware encoder
- [x] The Agent log names the encoder in use
- [x] `cargo test --workspace` and clippy clean

## Comments

- Encoder choice lives in `crates/agent/src/encoder.rs` (`H264::open`): `hwcodec` encoders from `HARDWARE` (macOS:
  `h264_videotoolbox`) when `Platform::hardware_encoder()` allows, openh264 otherwise or when none starts. A hardware
  encoder that errors mid-stream is swapped for openh264. The Agent logs
  `live stream <id>: H.264 through <encoder> at <level>` each time it opens an encoder.
- `hwcodec` is pinned to rustdesk-org/hwcodec `ec38eba` and builds against RustDesk's static FFmpeg from vcpkg
  (`VCPKG_ROOT`, see docs/usage.md). hwcodec itself only offers `hevc_videotoolbox` on macOS ("h264 encode failed too
  often"); we open `h264_videotoolbox` by name, which RustDesk's FFmpeg includes.
- Manual check (2026-11-11), MacBook Pro `MacBookPro18,3` (14-inch 2021, M1 Pro), macOS 15.8.1 (24H32): the live e2e
  tests stream through `h264_videotoolbox` (FakePlatform capture, real Agent encode path, openh264 decode in the
  viewer). At 3024x1964 the log named `h264_videotoolbox` at Balanced, Speed and Quality, measured 13 / 20.5 / 9 fps.
  Not checked: a real screen through the macOS adapter in a browser.
- VideoToolbox findings: output trails input by about 2 frames (asynchronous encode); frame sizes follow the level's
  bitrate as a ceiling, but on easy content VideoToolbox tops out in quality, so Quality and Balanced frames can be the
  same size. The level e2e test therefore runs on openh264 (`hardware_encoder = false`).
