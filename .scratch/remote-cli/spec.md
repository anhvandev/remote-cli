# Spec: Remote CLI: device management and AI-driven remote control

Status: ready-for-agent

Vocabulary follows `GLOSSARY.md`. Decisions respect `docs/adr/0001` (privileged Agent) and `docs/adr/0002` (RustDesk-based Agent, AGPL-3.0).

## Problem Statement

We run a fleet of up to ~100 machines spread across the internet, on Windows, macOS, Linux X11 and Linux Wayland. We cannot see at a glance which ones are up, what hardware they have, or how loaded they are, and we cannot wake them remotely. Most importantly, we want an AI agent to operate these machines on its own: move the mouse, type, run terminal commands, and see the result of every step. Existing remote-desktop tools are built for a human watching a live video stream. They don't give an AI a cheap, reliable way to act and then observe, they are heavy on bandwidth, and they stop working at the lock screen or after a reboot.

## Solution

A self-hosted system with three parts:

- **Server**: keeps the Device registry, Device Status and resource history. It relays Actions and handles enrollment, authentication, Control Locks, Wake and audit.
- **Agent**: installed on every Device. It reports inventory and resources and carries out Actions, including at the lock screen and login screen wherever the OS allows it.
- **Controller tools**:
  - a CLI (`rcli`), which is the core tool;
  - an MCP mode of the same CLI, so AI agents can call it as tools;
  - a Next.js Dashboard, where people can watch, control and use a terminal.

Every mouse or keyboard Action can return an Observation (none, one screenshot, change-triggered frames, or a short clip) so the Controller always sees what happened. Marks let the Controller target on-screen elements by number or text instead of guessing pixel coordinates. Heavy media goes peer-to-peer over WebRTC where possible, and everything is encoded to keep bandwidth low.

## User Stories

### Fleet visibility

1. As an operator, I want to list all Devices with their Device Status (Online, Offline, Waking), so that I know which machines I can work with right now.
2. As an operator, I want to see each Device's inventory (OS and version, hostname, CPU model and cores, total RAM, disks and capacity, GPU, network interfaces with IP/MAC, serial number, display resolutions), so that I know what hardware I'm dealing with.
3. As an operator, I want to see each Device's current resources (CPU %, RAM used, disk usage %, disk read/write rate, network up/down rate, GPU %, uptime, logged-in user, screen-lock state, and CPU temperature and battery where available), so that I can spot overloaded or stuck machines.
4. As an operator, I want resource history at 5-second resolution for the last 7 days and 1-minute resolution for the last 90 days, so that I can investigate what happened over time.
5. As an operator, I want resource history charts in the Dashboard, so that I can read trends visually.
6. As an operator, I want every Dashboard figure, including history, to be available from the CLI, so that scripts and AI agents get the same data people see.
7. As an operator, I want to request a Device's process list on demand, so that I can diagnose it without the Agent streaming process lists all the time.
8. As an operator, I want history to have no gaps when an Agent briefly loses the Server, so that short network drops don't hide what happened.
9. As an operator, I want inventory to update automatically when hardware or displays change, so that the registry stays accurate without manual refreshes.
10. As an operator, I want to see whether an Online Device is at the lock screen or the login screen and who is logged in, so that I know what an AI agent will face before it starts.

### Enrollment and Device lifecycle

11. As an admin, I want to create a single-use Enrollment Code, so that only machines I authorize can join.
12. As an admin, I want to install the Agent with an Enrollment Code and have the Device appear in the registry, so that onboarding a machine is one step.
13. As an admin, I want each Agent to receive its own credential at enrollment and the Enrollment Code to stop working afterwards, so that a leaked code cannot enroll a second machine.
14. As an admin, I want to revoke a Device's credential, so that a stolen or retired machine can no longer connect.
15. As an admin, I want a reinstalled machine to appear as a new Device rather than being matched automatically to an old one, so that cloned VMs or reused hardware never inherit the wrong identity.
16. As an admin, I want to merge an old Device's history into a new Device, so that I keep continuity after a reinstall when I know they're the same machine.
17. As an admin, I want to rename and delete Devices, so that the registry stays tidy.
18. As an admin, I want to store a Device's login password on the Server, so that Controllers can unlock it without ever knowing the password.

### Power and Wake

19. As an operator, I want to Wake an Offline Device, so that I can use machines that were switched off.
20. As an operator, I want the Server to send Wake through a Wake Relay on the same LAN automatically, so that I don't have to know the network layout.
21. As an operator, I want a Device to show Waking until it comes Online, and "Wake failed" if it doesn't within 3 minutes, so that I know whether the Wake worked.
22. As an operator, I want to be told clearly when a Device cannot be woken because no Wake Relay is online in its LAN, so that I can fix it (for example by leaving a small always-on machine there).
23. As an operator, I want to Restart, Shut down, Sleep or Lock the screen of a Device, so that I can manage power remotely.
24. As the Controller holding a Device's Control Lock, I want Restart, Shut down and Sleep to be refused to other operators (admins excepted), so that nobody pulls the machine out from under my session.

### Control Lock

25. As a Controller, I want to acquire the Control Lock on a Device before sending mouse or keyboard Actions, so that two Controllers never fight over the same mouse.
26. As a Controller without the Control Lock, I want an error that names the holder and when the lock expires, so that I can retry later.
27. As a Controller without the Control Lock, I want to keep observing the screen, reading metrics and using the terminal, so that only input is exclusive.
28. As the Control Lock holder, I want any Action I send, including observation-only requests, to reset the 10-minute idle timer, so that I don't lose the lock while I'm thinking.
29. As the Control Lock holder, I want my lock to survive a disconnect of up to 60 seconds, so that a brief network blip doesn't hand my Device to someone else.
30. As an operator, I want a lock whose holder disconnected for more than 60 seconds or went 10 minutes without an Action to be released automatically, so that crashed AI agents don't block a Device forever.
31. As an admin, I want to force-release a Control Lock, with the release recorded in the audit log, so that I can recover from a misbehaving holder.
32. As a person in the Dashboard, I want to see who holds a Device's Control Lock and when it expires, so that I know whether I can take over.
33. As a person in the Dashboard, I want to wait for the Control Lock to be released like any other Controller rather than seize it, so that an AI's in-progress work isn't interrupted mid-step.

### Actions and Observations

34. As a Controller, I want to move the mouse, click, double-click, right-click, drag, scroll, type text and press key combinations, so that I can operate any application.
35. As a Controller, I want to send an Action Batch (an ordered list of Actions plus waits) in one request and get one Observation back, so that multi-step interactions cost one round trip.
36. As a Controller, I want to choose the Observation mode per Action or Action Batch (`none`, `shot`, `frames`, `clip`), so that I pay only for the feedback I need.
37. As a Controller, I want `shot` to wait until the screen settles (up to about one second) before capturing, so that I don't see half-rendered screens.
38. As an AI Controller, I want `frames` to return only frames where the screen changed noticeably, capped at a maximum count and timestamped, so that I can follow animations and continuous interactions without video.
39. As a human Controller, I want `clip` to return a short H.264 clip of the Action, so that I can review exactly what happened.
40. As a Controller, I want screenshots downscaled and compressed by default, so that Observations are small enough to send after every Action.
41. As a Controller, I want an Observation to say "unchanged" instead of resending an identical image, so that idle screens cost almost nothing.
42. As a Controller, I want to give coordinates in the Observation image's space and have the Agent convert them to real pixels, so that I never do scaling math.
43. As a Controller on a multi-display Device, I want coordinates of the form (display, x, y), screenshots of the display I last acted on by default, and one separate image per display when I ask for all displays, so that multi-monitor setups don't lose detail.
44. As a Controller, I want to list a Device's displays with their resolutions and arrangement, so that I can pick the right one.

### Marks

45. As an AI Controller, I want an Observation annotated with numbered Marks on detected UI elements, so that I can choose a target by looking at the image.
46. As an AI Controller, I want a list of Marks with number, text, role and bounding box, so that I can reason about the screen as structured data.
47. As an AI Controller, I want to click or type into a Mark by number or by visible text, so that I don't have to compute coordinates I might get wrong.
48. As an AI Controller, I want Marks to come from the accessibility tree when it's available and from OCR otherwise (lock screen, login screen, Wayland, games), so that Marks work everywhere.
49. As an AI Controller, I want a clear error when a Mark number is stale or a text target is ambiguous, so that I re-observe instead of clicking the wrong thing.

### Lock screen and login screen

50. As an AI Controller, I want to observe and control a Device at its lock screen, so that I can keep working after a machine locks itself.
51. As an AI Controller, I want to send an `unlock` Action that makes the Agent type the stored password, so that I can unlock without ever seeing the password.
52. As an AI Controller on Windows, I want to observe and control the login screen and UAC prompts, and to send Ctrl+Alt+Del, so that a rebooted or elevated Windows machine is still usable.
53. As an AI Controller on Linux X11, I want to observe and control the display-manager login screen, so that a rebooted Linux machine is still usable.
54. As an operator, I want macOS Devices at the login window to be observable, and the Dashboard and CLI to say clearly that input is unavailable there, so that I know the machine needs auto-login or one human login.
55. As an AI Controller on Linux Wayland, I want to observe and control the lock screen and the greeter, so that Wayland machines behave like the others (pending the Wayland probe; see Further Notes).

### Terminal

56. As a Controller, I want to run a one-shot command and get stdout, stderr and the exit code, with a timeout, so that AI agents can use the shell reliably.
57. As a Controller, I want an interactive shell session from the CLI, so that I can work like over SSH.
58. As a person in the Dashboard, I want a web terminal next to the remote screen, so that I can use the shell without leaving the browser.
59. As an operator, I want commands to run as the logged-in user by default, so that the Agent's root/SYSTEM privileges are not handed out casually.
60. As an admin, I want to run commands as root/SYSTEM, including when nobody is logged in, so that I can fix machines that are stuck before login.
61. As an operator, I want to be refused root/SYSTEM execution, so that only admins hold full power.

### Dashboard

62. As a person, I want to sign in to the Dashboard with a password and a TOTP code, so that a stolen password alone can't take over the fleet.
63. As a person in the Dashboard, I want to watch a Device's screen live, so that I can follow what an AI agent is doing.
64. As a person in the Dashboard, I want to control a Device directly with my own mouse and keyboard once I hold its Control Lock, so that I can step in by hand.
65. As a person in the Dashboard, I want buttons for Wake, Restart, Shut down, Sleep and Lock screen on each Device, so that common tasks are one click.

### AI integration

66. As an AI agent developer, I want `rcli mcp` to expose observe, act, Action Batch, Marks, terminal, Device listing, metrics and Control Lock as MCP tools, with images returned inline, so that Claude Code and other agents can drive Devices directly.
67. As an AI agent developer, I want CLI output to be available as JSON, so that agents and scripts can parse it reliably.

### Security, audit and operations

68. As an admin, I want users and per-user API tokens with the roles admin and operator, so that each person and each AI agent has its own identity.
69. As an admin, I want every Action recorded in an audit log (who, when, which Device, parameters, terminal command and exit code) for 90 days, with passwords and `unlock` input masked, so that I can trace what happened.
70. As an admin, I want Observations and clips relayed but not stored, so that audit doesn't fill the disk with screen captures.
71. As a person sitting at a Device, I want a visible indicator while the Device is being controlled, so that remote control never happens invisibly.
72. As an admin, I want Agents to update themselves from the Server, either fleet-wide or per Device, and only from signed builds, so that I can roll out fixes to 100 machines without risking a malicious update.
73. As an admin, I want to deploy the Server, the database and the TURN relay with one Docker Compose file on a single VPS, so that operating the system is simple.
74. As a Controller, I want screen media to travel peer-to-peer and fall back to the TURN relay only when NAT blocks a direct path, so that latency and VPS bandwidth stay low.

## Implementation Decisions

### Shape and technology

- **Languages:** Rust for Agent, Server and CLI. A shared protocol crate defines every message between them. Dashboard in Next.js (TypeScript). License AGPL-3.0 (ADR 0002).
- **Storage:** Postgres for everything persistent: registry, users, tokens, Control Lock state, metrics, audit, encrypted unlock passwords.
- **Deployment:** one Docker Compose stack on one VPS with Server, Postgres, coturn and the Dashboard. No clustering.
- **Connections:**
  - The Agent opens a persistent outbound TLS connection to the Server and authenticates with its mTLS client certificate.
  - Commands, metrics and control messages travel on that connection as binary protobuf messages.
  - Screen media (live stream, clips, large Observations) goes over WebRTC, peer-to-peer between Agent and Controller, with coturn as fallback. The Server acts as WebRTC signaling.
- **Controller API:** the Server exposes one API used by CLI, MCP and Dashboard alike: HTTP for request/response, plus streaming channels for live screen, interactive terminal and frames. Authentication is a per-user API token for CLI/MCP and a session with TOTP for the Dashboard. Errors are typed. Most important: "Control Lock held", which carries the holder and expiry time.

### Server modules

- **Device Registry:**
  - Owns Devices, inventory and Device Status.
  - Online while connected. Offline after 30 seconds without a connection. Waking after a Wake, returning to Offline with "Wake failed" after 3 minutes.
  - Supports rename, delete, and merging an old Device's history into a new one.
- **Enrollment & Credentials:**
  - Issues single-use Enrollment Codes and exchanges each for a per-Device mTLS certificate.
  - Revokes certificates.
  - No automatic re-identification by serial or MAC.
- **Control Lock:**
  - One holder per Device.
  - Ends on release, after 10 minutes without any Action from the holder, after 60 seconds of holder disconnection, or on admin force-release (audited).
  - No pre-emption and no queue.
  - Uses an injectable clock so timeouts are testable.
- **Action Router:**
  - Checks the Control Lock for input Actions and for Restart/Shutdown/Sleep. Lock screen and observe/read Actions need no lock.
  - Checks roles, forwards to the Agent and returns the Observation.
  - Marks the Action in the audit log.
- **Metrics Store:**
  - Ingests 5-second samples and accepts Agent backfill.
  - Rolls up to 1-minute aggregates. Retention: raw 7 days, rolled-up 90 days.
  - Answers history queries for both Dashboard and CLI.
- **Wake Coordinator:** picks a Wake Relay, meaning an Online Agent in the same LAN as the target (matched on subnet), asks it to send the magic packet to the target's MAC, and tracks the Waking outcome. If no relay is available, the Wake fails immediately with a reason.
- **Secrets Vault:** stores per-Device login passwords encrypted with a server master key. It hands a password only to the target Agent for `unlock` and never returns it through the Controller API.
- **Audit Log:** records every Action's metadata for 90 days, masks secrets, and stores no media.
- **Auth:** users, roles (admin, operator), API tokens, Dashboard password + TOTP. There are no per-Device permissions.
- **Update Distribution:** serves signed Agent builds and lets an admin target all Devices or selected ones.

### Agent modules

- **Platform port.** The single seam between the Agent core and the OS. It covers:
  - displays (list, capture a frame per display)
  - input (pointer, keys, text)
  - session state (logged in, locked, login screen, who is logged in)
  - inventory and resource sampling, process list
  - power (restart, shutdown, sleep, lock)
  - shell (one-shot and PTY, as user or as root/SYSTEM)
  - Wake packet sending
  - accessibility tree

  Adapters exist for Windows, macOS, Linux X11 and Linux Wayland.
- **Capture and input reuse:** RustDesk's `scrap` and `enigo`, through a git dependency pinned to a commit (ADR 0002).
- **Lock/login-screen orchestration:** ported and rewritten compactly from RustDesk.
  - **Windows:** a SYSTEM service launches a helper in the active console session using the winlogon token. The helper switches to the input desktop before capture and input, rebuilds capture when the desktop changes, and uses SendSAS for Ctrl+Alt+Del, which requires the software SAS policy.
  - **macOS:**
    - A root LaunchDaemon, plus a LaunchAgent limited to the LoginWindow and Aqua sessions.
    - Screen Recording and Accessibility must be granted once at install.
    - At the login window, capture only. Input is reported as unavailable (proven by the prototype, ADR 0001).
    - At the lock screen, capture and input both work.
  - **Linux X11:** a root service finds the seat0 greeter's X server and its auth file and runs the helper as the greeter user. Input uses XTest.
  - **Linux Wayland:**
    - Logged-in sessions capture through the ScreenCast portal + PipeWire with a persisted restore token (one approval at install) and inject input through uinput.
    - For the lock screen and greeter, the candidate is DRM/KMS capture by root plus uinput. It is pending the Wayland probe.
- **Executor:** runs single Actions and Action Batches in order, then produces one Observation.
- **Observer:** produces Observations.
  - **Settle detection** for `shot`.
  - **Change-triggered, capped, timestamped frames** for `frames`.
  - **H.264** for `clip`.
  - **Encoding:** downscale (default long edge about 1280 px), WebP/JPEG compression, and "unchanged" when nothing differs from the previous Observation.
  - **Live stream:** WebRTC.
- **Coordinate Mapper:** converts (display, x, y) in Observation image space to real pixels on that display. All coordinate math lives here, never in the Controller.
- **Mark Detector:** detects Marks from the accessibility tree, falling back to OCR. It annotates Observations with numbered Marks, keeps the Mark table for the latest Observation, and resolves "Mark N" or "text T" into a coordinate. Stale or ambiguous targets are errors.
- **Metrics Collector:** sends inventory at start and on change, and resources every 5 seconds as deltas. It buffers up to 1 hour locally while disconnected and backfills on reconnect. The process list is only sent on request.
- **Updater:** downloads Agent builds from the Server, verifies the signature, installs, and restarts.
- **Control indicator:** shows a visible on-Device indicator while a Controller holds the Control Lock or a live stream is active.

### Controller tools

- **CLI (`rcli`):** a thin client over the Controller API. Commands cover:
  - Devices: list, inventory, metrics and history
  - power and Wake
  - Control Lock: acquire, release, status
  - Actions and Action Batches with an Observation mode flag
  - Marks
  - `exec` (one-shot) and `shell` (interactive)
  - admin: users, tokens, Enrollment Codes, Devices, unlock passwords, updates

  Machine-readable JSON output is always available.
- **MCP:** `rcli mcp` serves the same operations as MCP tools and returns Observations as inline images.
- **Dashboard (Next.js):** sign-in with TOTP; fleet list with Device Status; inventory and history charts; live view; direct control while holding the Control Lock; web terminal; power and Wake buttons; admin screens.

## Testing Decisions

- **What a good test is:** it drives the system only through the Controller API and asserts on what comes back (responses, errors, Observations, metrics, audit entries). It never reaches into internal module state. Tests survive refactors of anything behind the seam.
- **Primary seam: the Controller API, with the whole system in-process.**
  - Each test runs a real Server on Postgres in a test container, plus one or more real Agents whose Platform port is a **FakePlatform**. A test client calls the Controller API.
  - The FakePlatform serves scripted screens: images with known elements, an accessibility tree, and a lock/login state. It records injected input and returns scripted metrics and shell results.
  - Time is controlled through the injectable clock.
  - This seam covers:
    - enrollment and revocation
    - Device Status transitions and Wake outcomes, including no Wake Relay
    - Control Lock rules: idle timeout, disconnect grace, no pre-emption, admin release, error contents
    - roles and `--as root` refusal
    - Action Batch ordering
    - Observation modes, settle and "unchanged"
    - coordinate mapping across displays and scales
    - Mark resolution, including stale and ambiguous targets
    - `unlock` never leaking the password, and audit masking
    - metrics ingestion, backfill after a disconnect, rollup and retention
    - signed-update acceptance and rejection
- **Platform adapters:** contract tests run on real machines per OS, in the style of the lockscreen probe. They check that the Agent can capture and that an injected key appears, logged in, at the lock screen and at the login screen. They also check that session-state detection matches reality. These are run manually or on dedicated per-OS runners, not in the main CI.
- **CLI and MCP:** a small set of smoke tests against the in-process system: one happy path per command group, plus JSON output shape.
- **Dashboard:** a few Playwright tests against the in-process system with FakePlatform: TOTP sign-in, fleet list, waiting for and taking the Control Lock, live view, and opening the web terminal.
- **Prior art:** none in this repo yet; this is greenfield. `prototype/lockscreen-probe/` is the reference for the shape of the platform contract tests.

## Out of Scope

- Multi-tenant separation and per-Device permissions.
- A queue for the Control Lock; pre-emption of the Control Lock by people or admins (only admin force-release).
- Keyboard and mouse input at the macOS login window, and FileVault pre-boot unlock.
- Storing Observations, clips or live streams in the audit log.
- File transfer, clipboard sync, audio, and printing.
- Alerting and notifications (offline alerts, threshold alerts).
- Mobile Devices (Android, iOS).
- Server clustering or high availability.

## Further Notes

- **Wayland (pending):**
  - Lock screen and greeter support depend on `prototype/lockscreen-probe/linux`, which the user will run on GNOME, KDE and wlroots. Until then, user story 55 and the Wayland lock/greeter part of the Platform port are provisional.
  - If KMS capture plus uinput works, it covers both the lock screen and the greeter regardless of compositor. If it fails, Wayland drops to "logged-in sessions only", and the Dashboard must say so the way it does for the macOS login window.
  - Known risks: NVIDIA proprietary drivers and missing VA-API break the KMS path. RustDesk's greeter detection does not recognise the newer GDM greeter user.
- **macOS:** the probe (macOS 15.8.1) showed:
  - Capture works at the login window once Screen Recording is granted.
  - Every CGEvent injection variant fails at the login window. The HID-system-state variant hangs.
  - At the lock screen, capture and keyboard input both work.
  - The OS shows "Your screen is being observed" while capturing.
- **Licensing:** RustDesk's `scrap` Wayland code derives from Weylus (AGPL-3.0+), and `hbb_common` has no LICENSE file. Both fit an AGPL-3.0 project but should be noted in the project's license notices.
- **Research sources:** `docs/research/lock-screen-and-prior-art.md` and `docs/research/rustdesk-capabilities.md`.
