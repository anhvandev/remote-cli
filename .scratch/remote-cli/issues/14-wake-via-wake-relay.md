# 14: Wake via Wake Relay

**What to build:** An operator Wakes an Offline Device. The Server picks a Wake Relay, an Online Agent on the same LAN (matched on subnet), and has it send the magic packet to the target's MAC. The Device shows Waking until it comes Online. It returns to Offline with "Wake failed" after 3 minutes. With no relay available, the Wake fails immediately with the reason.

**Blocked by:** 03, 06

**Status:** ready-for-agent

- [ ] `rcli wake <device>`
- [ ] Wake Relay chosen from Online Agents sharing the target's subnet; it sends the packet to the target's MAC
- [ ] Waking → Online, or Waking → Offline with "Wake failed" after 3 minutes (injectable clock)
- [ ] No relay available → immediate failure explaining why
- [ ] Wake audited
