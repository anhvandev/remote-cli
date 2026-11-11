# 33: Linux X11: root service and greeter

**What to build:** Run the Agent as a root systemd service that finds the seat0 greeter's X server and auth file, and runs the helper as the greeter user. The display-manager login screen and the lock screen can then be observed and controlled. Ship deb/rpm packages that take an Enrollment Code.

**Blocked by:** 32

**Status:** wontfix

- [ ] Packages enroll the Device and install the service
- [ ] Contract suite passes at the lock screen and at the GDM/LightDM/SDDM greeter
- [ ] `unlock` works at both

## Comments

- 2026-11-09: Closed. Superseded by `.scratch/easy-setup/issues/16-linux-service-install-greeter.md` (new install flow: Server Key, Device ID + Device Password, ADR 0003/0004).
