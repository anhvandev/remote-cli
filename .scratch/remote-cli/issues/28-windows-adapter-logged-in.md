# 28: Windows adapter: logged-in session

**What to build:** Implement the Platform port for Windows in a logged-in session. Capture and input reuse RustDesk's scrap/enigo (git dependency pinned to a commit, ADR 0002). Also covers the accessibility tree via UI Automation, inventory and resources, shell, power, Wake send and the control indicator.

**Blocked by:** 27

**Status:** ready-for-agent

- [ ] Platform contract suite passes on a real Windows 10/11 machine in a logged-in session
- [ ] Multi-display capture and input
- [ ] UI Automation tree feeds Marks
