# 05: Install scripts, Agent GUI and Connect guide use `rcli install`

**What to build:** Every install route runs the single `rcli install`: the Linux/macOS `curl … | sudo sh` script, the Windows PowerShell script, the Install button in the Agent GUI (downloaded `rcli.exe`), and the headless command. The Connect guide shows only the install commands and `rcli connect`, with no "Thiết lập Controller" section and no `<Controller API URL>` placeholder; its MCP section needs only `rcli connect`. The usage and deploy docs describe one install. See `.scratch/setup-v2/spec.md`.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] Both install scripts and the GUI Install button run `rcli install` and print the Device ID and Device Password
- [ ] After a scripted install, `rcli connect` works on that machine
- [ ] The Connect guide has no Controller setup section or placeholder
- [ ] The docs mention neither `rcli setup` nor `rcli agent install`
- [ ] `cargo test --workspace`, clippy and `cd dashboard && npx tsc --noEmit` clean; Playwright install-scripts and connect-guide specs pass

## Comments
