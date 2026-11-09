# 23: macOS adapter: logged-in session

**What to build:** Carried over from `.scratch/remote-cli/issues/30`. Platform port for macOS in a logged-in session. Capture and input reuse scrap/enigo. Also covers the accessibility tree via the AX API, inventory and resources, shell, power and Wake send. The user runs the contract suite on a real machine.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Platform contract suite passes on a real macOS 15 machine in a logged-in session
- [ ] Multi-display and Retina scaling handled by the Coordinate Mapper
- [ ] AX tree feeds Marks
