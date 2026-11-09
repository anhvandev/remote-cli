# 14: Agent GUI window

**What to build:** One small egui window that is a client of the local IPC. It shows the Device ID; reveals, copies and changes the Device Password after an OS elevation prompt (polkit, UAC, macOS authorization); shows connection status and Server Address/Key; edits Device Settings (Terminal User, root level, accept mode) and the unlock password; shows 'being controlled by …' with Disconnect and 'grant root to this session'; and shows the prompts from tickets 07 and 08. No tray icon. It opens from the app menu or when `rcli` is run with no arguments on a desktop, and the service opens it when someone starts controlling the Device. The binary must still start on machines without display libraries. This window is the control indicator for the platform tickets. See `.scratch/easy-setup/spec.md` (GUI).

**Blocked by:** 07, 08, 09

**Status:** ready-for-agent

- [ ] Manual checklist on a Linux desktop: every item above works and applies live
- [ ] Password reveal needs elevation; Disconnect does not
- [ ] `rcli agent …` still works on a headless machine with no GUI libraries
