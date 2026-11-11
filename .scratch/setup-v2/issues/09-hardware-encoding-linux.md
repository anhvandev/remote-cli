# 09: Hardware encoding for Live View on Linux

**What to build:** On Linux, Live View encodes through `hwcodec`'s VA-API encoder, with the software fallback from 07, on X11 and Wayland. Live View keeps working at the greeter and lock screen; if the hardware encoder cannot start there, it falls back to openh264. See `.scratch/setup-v2/spec.md`.

**Blocked by:** 07

**Status:** ready-for-agent

- [ ] On Linux with a VA-API GPU, Live View uses it on X11 and on Wayland (manual check; record GPU, distro and session type)
- [ ] Live View works at the greeter and lock screen (manual check; record which encoder ran)
- [ ] A machine without VA-API streams through openh264
- [ ] `cargo test --workspace` and clippy clean

## Comments
