# 30: macOS adapter: logged-in session

**What to build:** Implement the Platform port for macOS in a logged-in session. Capture and input reuse scrap/enigo. Also covers the accessibility tree via the AX API, inventory and resources, shell, power, Wake send and the control indicator.

**Blocked by:** 27

**Status:** wontfix

- [ ] Platform contract suite passes on a real macOS 15 machine in a logged-in session
- [ ] Multi-display and Retina scaling handled by the Coordinate Mapper
- [ ] AX tree feeds Marks

## Comments

- 2026-11-09: Closed. Superseded by `.scratch/easy-setup/issues/23-macos-adapter-logged-in.md` (new install flow: Server Key, Device ID + Device Password, ADR 0003/0004).
