# Setup v2: one install command, no stored OS passwords, native-resolution Live View

**Status:** ready-for-agent

## Problem Statement

Setting up rcli still confuses people. The guide shows two different commands, one for "the machine being controlled" and one for "the controlling machine". The second one needs a separate `https://` URL that the Connect guide cannot even fill in. People also believe rcli takes over the Device's OS login password, and in fact it can: an unlock password can be stored on the Device and typed by the Agent. That contradicts what the product is supposed to promise, which is that it keeps nothing about the Device's accounts. Finally, a person watching a Device in Live View gets a picture downscaled to 1280 px at a fixed 10 fps, with no way to trade sharpness for smoothness. Small text on large or Retina screens is unreadable.

## Solution

- **One command on every machine.** `rcli install --server <Server Address> --key <Server Key>` turns any machine into a Device (it prints its Device ID and Device Password) and at the same time lets it control other Devices, the way TeamViewer or RustDesk work. The user types only the Server Address and the Server Key. The Server hands out the API Address itself and learns it on its own: the Controller API is served through the Dashboard's public URL, and the Server records that URL the first time an Admin opens the Dashboard over `https://`.
- **No stored OS passwords.** rcli stores no OS login password anywhere. At a lock or login screen, the Controller (a person or an AI agent) types the password itself as a secret `type` Action, and audit masks it.
- **Native-resolution Live View.** Live View streams the Device's screen at its native resolution. Each viewer picks a Live View Quality (Quality, Balanced or Speed) and can switch it mid-session. Encoding uses the GPU when there is one and falls back to software otherwise. Observations for AI agents stay as they are.

## User Stories

### One install command

1. As a person setting up a machine, I want one install command, so that I don't have to work out whether my machine is "an agent" or "a controller".
2. As a person setting up a machine, I want to type only the Server Address and the Server Key, so that setup is a single copy-paste.
3. As a person setting up a machine, I want the install to print the Device ID and Device Password, so that I can give them to whoever will control this Device.
4. As a person who has just installed rcli, I want `rcli connect <Device ID>` to work right away, so that I can control another Device without a separate setup step.
5. As a person who has just installed rcli, I want `rcli mcp` to work right away, so that my AI agent can drive other Devices with no extra configuration.
6. As a second local user on a machine where rcli is installed, I want Controller commands to work without reinstalling, so that installing is a once-per-machine step.
7. As a local user, I want my remembered Device Passwords to stay private to my account, so that other users on the same machine cannot connect as me.
8. As a person installing on Windows, I want the PowerShell one-liner, the downloaded `rcli.exe` GUI and the elevated command to do the same single install, so that every route gives the same result.
9. As a person installing on Linux or macOS, I want the `curl … | sudo sh` one-liner to do the same single install, so that every route gives the same result.
10. As a person installing on a headless Device, I want the same `rcli install` to work without a display, so that servers need no special path.
11. As a person who mistyped the Server Key, I want the install to fail immediately and leave nothing behind, so that I can retry cleanly.
12. As a person installing before an Admin has ever opened the Dashboard over `https://`, I want the install to succeed as a Device, so that setup is not blocked.
13. As a person in that situation, I want Controller commands to tell me clearly that the Server has no API Address yet, so that I know whom to ask instead of seeing a connection error.
14. As a person removing rcli, I want `rcli uninstall` to remove both the Agent and the machine-wide Controller configuration, so that one command undoes one install.
15. As a person removing rcli, I want my remembered Device Passwords kept, so that reinstalling does not make me re-enter them.
16. As an Admin, I want the Connect guide to show only the install commands and `rcli connect`, so that there is no "Thiết lập Controller" section or `<Controller API URL>` placeholder to confuse people.
17. As an Admin, I want `rcli setup`, `rcli agent install` and `rcli agent uninstall` gone, so that old instructions fail loudly instead of half-working.

### API Address

18. As an Admin, I want the Server to learn its API Address automatically from my first `https://` visit to the Dashboard, so that I never configure a URL by hand.
19. As an Admin, I want visits over `http://`, to `localhost` or to a private IP to never change the API Address, so that a test visit on the internal port cannot break every install.
20. As an Admin, I want to see the API Address in Server Settings and correct it, so that I can fix it if it was learned wrong or the public URL moves.
21. As an Admin, I want a corrected API Address to reach every Agent the next time it connects, so that I don't touch any Device.
22. As an Admin deploying the Server, I want only the Dashboard port and the Agent port exposed to the internet, so that there is one fewer port and one fewer TLS certificate to manage.
23. As a Controller using the CLI or MCP, I want every Controller API call to work through the Dashboard URL, including the shell stream and Control Lock watching, so that nothing depends on the internal API port.
24. As a person using the web terminal, I want it to keep working through the Dashboard, so that the move is invisible to me.

### No stored OS passwords

25. As the owner of a Device, I want rcli to never store my OS login password, neither on the Server nor on the Device, so that installing rcli does not put my account credentials at risk.
26. As a Controller, I want to still see and control the lock screen and login screen, so that Devices stay reachable after they lock or reboot.
27. As a Controller, I want to type the OS password at a lock screen with a `type` Action marked `secret`, so that I can unlock the Device myself.
28. As an auditor, I want any text typed with `secret`, or typed while the Device is at a lock or login screen, to appear masked in the audit log, so that passwords never end up in the audit.
29. As an AI agent using `rcli mcp`, I want the MCP instructions to tell me to type the OS password the user gave me as a secret `type` at a lock or login screen, so that I know rcli will not unlock the Device for me.
30. As the owner of a Device that once stored an unlock password, I want the upgraded Agent to delete it on startup, so that no old copy remains on the disk.
31. As a person at the Device, I want the Agent GUI to have no unlock password section, so that nothing suggests rcli keeps my password.
32. As a Controller, I want `rcli unlock` and the unlock endpoint gone, so that there is exactly one way to get past a lock screen.
33. As an operator of a mixed-version fleet during a rollout, I want old and new Agents and Servers to keep talking while the unlock field is removed, so that the upgrade does not break connections.
34. As a Mac owner, I want to know that the macOS login window stays view-only, so that I enable auto-login on Macs that must survive a reboot.

### Live View Quality

35. As a person watching a Device in Live View, I want the stream at the screen's native resolution, so that small text is readable.
36. As a person watching a Retina or HiDPI screen, I want native to mean physical pixels, so that the picture is as sharp as the real screen.
37. As a person watching, I want to pick Quality, Balanced or Speed from the Live View toolbar, so that I can trade sharpness for smoothness.
38. As a person watching, I want Balanced as the default, so that a new session looks reasonable without any choice.
39. As a person watching, I want to switch the level mid-session without reconnecting, so that I don't lose my place or my Control Lock.
40. As a person watching, I want my level to affect only my own stream, so that another viewer of the same Device keeps theirs.
41. As a person watching over a slow link, I want Speed to favour frame rate at a lower bitrate, so that mouse and keyboard feel responsive.
42. As a person reading fine text, I want Quality to favour sharpness at a higher bitrate, so that I can read the screen.
43. As a person holding the Control Lock, I want clicks and keystrokes on the native-resolution video to land where I point, so that direct control stays accurate.
44. As a person watching a Device with several displays, I want each display streamed at its own native size, so that mixed-resolution setups look right.
45. As an AI agent, I want Observations to stay at their current 1280 px long edge, so that coordinates and token cost stay as they are.

### Hardware encoding

46. As the owner of a Device with a GPU, I want Live View encoded on the GPU, so that streaming native resolution does not eat the CPU.
47. As the owner of a virtual machine or a headless Device, I want Live View to fall back to software encoding silently, so that it still works without a GPU.
48. As a person watching a lock or login screen, I want Live View to keep working even if the GPU encoder cannot start in that session, so that unattended control still works.
49. As a person debugging a Device, I want the Agent log to name the encoder in use, so that I can tell hardware from software encoding.

## Implementation Decisions

- **ADR 0005, no stored OS passwords.** Remove the unlock password end to end:
  - the Agent typing a stored password
  - the Secret Store entry and its set/clear IPC requests, and the root-only "unlock password set" flag in the Agent's `info`
  - the Agent GUI section
  - `rcli agent unlock-password` and `rcli unlock`
  - the Server's unlock endpoint and its "not set" error
  - the `unlock` field of the Agent protocol's act request

  The protocol field number is marked reserved. On startup an Agent deletes any leftover unlock password from its Secret Store. Getting past a lock screen relies only on the existing `type` Action with `secret`, and the existing audit masking already covers it. The MCP server instructions gain one sentence telling the AI agent to type the OS password itself.
- **ADR 0006, every install is a Device and a Controller.**
  - `rcli install` replaces `rcli agent install` and `rcli setup`, and `rcli uninstall` replaces `rcli agent uninstall`. There are no aliases.
  - Install writes a machine-wide Controller configuration, read-only for local users: Server Address, Server Key, pinned update key and API Address. Remembered Device Passwords stay per user, in the OS keychain or a 0600 file as today.
  - Controller commands read the machine-wide configuration. With no API Address yet, they fail with a clear message.
  - The install scripts, the Agent GUI's Install button, the Connect guide and the docs all switch to the single command.
- **API Address.**
  - The Dashboard forwards the whole Controller API path, including streaming responses (shell, Control Lock watch), to the internal Controller API, the same way it already proxies the install scripts and the web terminal.
  - The Server stores one API Address in its settings. It records it from an Admin's Dashboard request whose forwarded protocol is `https` and whose host is neither localhost nor a private or loopback IP. Only requests from trusted proxies count, as with the existing forwarded-for handling.
  - An Admin can read and correct the API Address in Server Settings.
  - The Server sends the API Address to Agents at registration and on every connection, so a change propagates without touching Devices.
  - The Controller API port no longer needs to face the internet.
- **Live View Quality.**
  - Live View streams each display at its native physical pixel size, rounded to even dimensions for H.264. Observations keep their 1280 px long edge.
  - The three levels differ only in bitrate and frame rate: Quality is high bitrate at about 10 fps, Balanced is medium at about 15 fps, Speed is low at about 25 fps. Exact numbers are tuned during implementation.
  - The Live View session accepts a level change mid-stream from its viewer. Each viewer already has its own stream, so levels are per viewer. The default is Balanced, and the level is not remembered between sessions.
  - The Dashboard Live View toolbar gets a picker.
  - Coordinate mapping for direct control follows the streamed size.
- **Hardware encoding.** Live View encodes H.264 through RustDesk's `hwcodec` (in line with ADR 0002): Media Foundation / NVENC / QSV / AMF on Windows, VideoToolbox on macOS, VA-API on Linux. If no hardware encoder exists or it fails to start, the Agent falls back silently to openh264 and logs which encoder is in use. The Dashboard shows no HW/SW indicator.
- **Glossary.** The spec uses the resolved terms: API Address, Live View Quality, the narrowed Secret Store, and Device Password (never "login password").
- **Tickets** under `issues/`: 01 remove the unlock password; 02 Controller API through the Dashboard URL; 03 the Server learns its API Address; 04 `rcli install` / `rcli uninstall` (blocked by 02, 03); 05 install routes and Connect guide (blocked by 04); 06 native resolution and Live View Quality; 07 hardware encoding on macOS with fallback (blocked by 06); 08 Windows and 09 Linux hardware encoding (blocked by 07).

## Testing Decisions

- Tests check external behaviour only: what a Controller, a viewer, an Admin or the audit log can observe. They do not check how the Agent or Server is structured inside.
- **Primary seam: the existing end-to-end test system.** It runs a real Server with Agents on the fake platform, driven through `rcli`, `rcli mcp` and HTTP. It covers:
  - the unlock password removal: an upgraded Agent deletes a leftover entry, and secret and lock-screen typing stays masked in audit
  - native-size Live View frames, using the existing live viewer helper and a fake display larger than 1280 px
  - Observations staying at 1280 px
  - level switching mid-session, and two viewers on different levels
  - the forced software fallback
  - API Address learning, filtering and propagation to Agents
  - `rcli install` / `connect` / `mcp` / `uninstall` against the machine-wide configuration
  - per-user privacy of remembered passwords
  - the clear message when the API Address is missing

  Prior art: the existing tests for the unlock password, live view, registration, MCP, install scripts and settings. One small helper may be added to send a request as if it came through a trusted proxy over `https` from a public host.
- **Secondary seam: Playwright against the dev stack**, only for behaviour that lives in the Dashboard:
  - the Live View Quality picker
  - the Controller API, including the shell stream, working through the Dashboard URL
  - reading and correcting the API Address in Server Settings
  - the Connect guide showing only the install commands and `rcli connect`

  Prior art: the existing live view, terminal, server settings and Connect guide specs.
- **Manual checks on real hardware** for what the fake platform cannot show: hardware encoding on Windows, macOS and Linux, and Live View at the lock and login screens on Windows and Linux. Each ticket records which of these were verified.

## Out of Scope

- A desktop viewer window inside the rcli app. Live View stays in the Dashboard.
- Changing Observation size, format or compression for AI agents.
- A `--controller-only` install that does not make the machine a Device.
- Calling the Controller API through the Agent port with the Device's mTLS certificate.
- Naming Controllers in audit by their Device ID instead of machine name and IP.
- An HW/SW encoder indicator in the Dashboard, and remembering the Live View Quality between sessions.
- Any way for rcli to get, keep or type an OS password on its own, including at the macOS login window, which stays view-only.
- Migrating existing installs. No Device has been installed yet.
- Codecs other than H.264.

## Further Notes

- Risk: every stream (shell, Control Lock watch) now passes through the Dashboard's Next.js server. Until now only the web terminal did this. Watch for buffering or compression on the reverse proxy, as `docs/deploy.md` already warns.
- Risk: hardware encoders may not start in the lock or login screen sessions (the Windows SYSTEM helper, the Linux greeter/KMS). The software fallback keeps Live View working there.
- Consequence: every machine that controls others, including one running an AI agent, can itself be controlled by anyone with its Device Password. Device Settings (accept each connection, root access) are what protect it (ADR 0004, ADR 0006).
- Related ADRs: 0001 (privileged Agent), 0002 (RustDesk-based Agent), 0003 (Server Key and Device Password), 0004 (Device Settings), 0005, 0006.
