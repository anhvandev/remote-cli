# 12: Devices without a display

**What to build:** An Agent with no display keeps metrics, exec, shell, power and Wake, and refuses shot, act, batch and Live View with a 'Device has no display' error. The Dashboard hides the Screen tab for such Devices and says why. The FakePlatform can be scripted with zero displays. See `.scratch/easy-setup/spec.md` (Headless Devices).

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Screen Actions return the 'no display' error; everything else works
- [ ] The Dashboard hides the Screen tab and explains why
