# 17: Install scripts and downloads served by the Server

**What to build:** The Server serves `install.sh`, `install.ps1` and per-OS/arch downloads of the currently rolled-out signed build, through the Dashboard origin. `curl -fsSL https://<dashboard>/install.sh | sudo sh -s -- --key <key>` downloads `rcli` and runs `rcli agent install`. `install.ps1` does the same on Windows and is finished in ticket 22. See `.scratch/easy-setup/spec.md` (Downloads and install scripts).

**Blocked by:** 16

**Status:** ready-for-agent

- [ ] The one-line install works end to end on a real Linux machine
- [ ] Clear error when no build is rolled out for that OS/arch
