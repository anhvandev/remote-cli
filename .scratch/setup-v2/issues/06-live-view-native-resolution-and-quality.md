# 06: Live View at native resolution with Live View Quality

**What to build:** Live View streams each display at its native physical pixel size (e.g. 2880×1800 for a Retina 1440×900 screen), rounded to even dimensions for H.264, instead of a 1280 px long edge. Observations keep their 1280 px long edge. A viewer picks a Live View Quality (GLOSSARY) from the Live View toolbar: Quality (high bitrate, ~10 fps), Balanced (default, medium bitrate, ~15 fps) or Speed (low bitrate, ~25 fps); exact numbers are tuned here. Switching mid-session takes effect without reconnecting and affects only that viewer's stream. The level is not remembered between sessions. Clicks and keystrokes from the Control Lock holder still land where they point. Software encoding (openh264). See `.scratch/setup-v2/spec.md`.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Live View frames have the display's native size for a display larger than 1280 px
- [ ] Observations still have a 1280 px long edge
- [ ] A level switch mid-session changes bitrate and frame rate without a new connection
- [ ] Two viewers of one Device can use different levels; the default is Balanced
- [ ] Direct control through Live View still targets the right pixel
- [ ] `cargo test --workspace`, clippy and `cd dashboard && npx tsc --noEmit` clean; Playwright live-view spec covers the picker

## Comments
