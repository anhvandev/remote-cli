# 07: Hardware encoding for Live View on macOS, with software fallback

**What to build:** Live View encodes H.264 through RustDesk's `hwcodec` (ADR 0002), starting with VideoToolbox on macOS. The three Live View Quality levels drive the hardware encoder's bitrate and frame rate. When no hardware encoder exists or it fails to start, the Agent silently falls back to openh264, and the Agent log names the encoder in use. No HW/SW indicator in the Dashboard. See `.scratch/setup-v2/spec.md`.

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] On a Mac, Live View uses VideoToolbox (manual check; record the machine and macOS version)
- [ ] With the hardware encoder forced unavailable, Live View still works through openh264 (test)
- [ ] All three Live View Quality levels work on the hardware encoder
- [ ] The Agent log names the encoder in use
- [ ] `cargo test --workspace` and clippy clean

## Comments
