# 34: Linux Wayland adapter: logged-in session

**What to build:** Implement the Platform port for Wayland sessions (GNOME, KDE, wlroots). Capture uses the ScreenCast portal + PipeWire with a persisted restore token, needing one approval at install. Input goes through uinput from the root service.

**Blocked by:** 27

**Status:** wontfix

- [ ] Contract suite passes logged in on GNOME and KDE; wlroots (Sway/Hyprland) result recorded
- [ ] Restore token persists across Agent restarts so no repeated approval is needed
- [ ] AT-SPI tree feeds Marks where available, OCR otherwise

## Comments

- 2026-11-09: Closed. Superseded by `.scratch/easy-setup/issues/19-linux-wayland-adapter-logged-in.md` (new install flow: Server Key, Device ID + Device Password, ADR 0003/0004).
