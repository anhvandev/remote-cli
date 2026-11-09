# 15: Linux X11 adapter: logged-in session

**What to build:** Carried over from `.scratch/remote-cli/issues/32`. Implement the Platform port for Linux X11 in a logged-in session. Capture and input reuse scrap/enigo (XTest). Also covers the accessibility tree via AT-SPI, inventory and resources, shell, power and Wake send. The control indicator is the GUI (ticket 14). The user runs the contract suite on a real machine, and the agent supplies the commands.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Platform contract suite passes on a real X11 desktop
- [ ] AT-SPI tree feeds Marks
