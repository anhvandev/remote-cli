# 07: Device Settings: Terminal User and root access

**What to build:** exec and shell run as the Device's Terminal User: by default the installing account (for now, the account running the Agent), changeable with `sudo rcli agent terminal-user set`. `sudo` inside the terminal works as it does over SSH. Root access is set with `sudo rcli agent root never|ask|always` (default `ask`). `--as root` on exec/shell asks the Device. `ask` raises a prompt naming the requester, with Accept/Reject and auto-reject after 30 s, and with no session to show it in, it is rejected. The person at the Device can grant root to the current session only. On the Controller, `rcli root on|off <ID>` makes `--as root` the default for that Device. Rules apply equally to password connections and Dashboard Users. Every root run and prompt answer is audited. The FakePlatform answers prompts from a script. See `.scratch/easy-setup/spec.md`, ADR 0004.

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] exec/shell run as the Terminal User
- [ ] never / ask / always behave as specified, including timeout and no-session rejection
- [ ] A session root grant ends with the session
- [ ] Dashboard Users are subject to the same rules
- [ ] `rcli root on <ID>` makes exec/shell request root by default
- [ ] Root runs and prompt answers are audited
