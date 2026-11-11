# 20: Linux Wayland: lock screen and greeter via KMS

**What to build:** Carried over from `.scratch/remote-cli/issues/35`. Make Wayland Devices observable and controllable at the lock screen and greeter with DRM/KMS capture by root plus uinput. **Blocked on the Wayland lockscreen probe**, which the user runs on GNOME, KDE and wlroots. If the probe fails, this ticket becomes: report 'input/capture unavailable at Wayland lock/greeter', the way the macOS login window does.

**Blocked by:** 19, plus results of the Wayland lockscreen probe (human)

**Status:** needs-info

- [ ] Wayland probe results recorded in the prototype README
- [ ] If KMS works: contract suite passes at the lock screen and greeter on the tested desktops; handles the newer GDM greeter user
- [ ] If not: the Device reports the limitation clearly in CLI and Dashboard
- [ ] Known limits (NVIDIA proprietary, no VA-API) documented

- 2026-11-11: Still `needs-info`, and the only blocker is the user's Wayland lock/greeter probe (`prototype/lockscreen-probe/README.md`, `linux/`). Ticket 19's logged-in Wayland adapter is merged, so once the probe result is recorded this ticket is either the KMS capture path or the "report unavailable" fallback; no agent-side work is blocked on it today.
