# 08: Hardware encoding for Live View on Windows

**What to build:** On Windows, Live View encodes through `hwcodec`'s Windows encoders (Media Foundation, NVENC, QSV or AMF, whichever the machine has), with the software fallback from 07. Live View keeps working at the lock screen, the login screen and the UAC prompt, where the Agent's SYSTEM helper captures; if the hardware encoder cannot start there, it falls back to openh264. See `.scratch/setup-v2/spec.md`.

**Blocked by:** 07

**Status:** ready-for-agent

- [ ] On Windows with a GPU, Live View uses a hardware encoder (manual check; record GPU and Windows version)
- [ ] Live View works at the lock screen, login screen and UAC prompt (manual check; record which encoder ran)
- [ ] A Windows VM without a GPU encoder streams through openh264
- [ ] `cargo test --workspace` and clippy clean

## Comments
