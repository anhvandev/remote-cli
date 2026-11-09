# 19: Linux Wayland adapter: logged-in session

**What to build:** Carried over from `.scratch/remote-cli/issues/34`. Platform port for Wayland sessions (GNOME, KDE, wlroots). Capture uses the ScreenCast portal + PipeWire with a persisted restore token, which needs one approval at install. Input goes through uinput from the root service. The user runs the contract suite on real machines.

**Blocked by:** 16

**Status:** ready-for-agent

- [ ] Contract suite passes logged in on GNOME and KDE; wlroots (Sway/Hyprland) result recorded
- [ ] The restore token persists across Agent restarts, so no repeated approval is needed
- [ ] AT-SPI tree feeds Marks where available, OCR otherwise
