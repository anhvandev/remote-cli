# 35: Linux Wayland: lock screen and greeter via KMS

**What to build:** Make Wayland Devices observable and controllable at the lock screen and the greeter. The candidate approach is DRM/KMS capture by root plus uinput input, which works regardless of compositor. **Blocked on the Wayland probe** (`prototype/lockscreen-probe/linux`), which the user will run on GNOME, KDE and wlroots. If the probe fails, this ticket becomes: report "input/capture unavailable at Wayland lock/greeter" the way macOS login window does.

**Blocked by:** 34, plus results of the Wayland lockscreen probe (human)

**Status:** wontfix

- [ ] Wayland probe results recorded in the prototype README
- [ ] If KMS works: contract suite passes at the lock screen and greeter on the tested desktops; handles the newer GDM greeter user
- [ ] If not: the Device reports the limitation clearly in CLI and Dashboard
- [ ] Known limits (NVIDIA proprietary, no VA-API) documented

## Comments

- 2026-11-09: Closed. Superseded by `.scratch/easy-setup/issues/20-linux-wayland-lock-greeter-kms.md` (new install flow: Server Key, Device ID + Device Password, ADR 0003/0004).
