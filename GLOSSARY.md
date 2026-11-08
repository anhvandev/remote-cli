# Remote CLI

Remote management and control of machines: track their state and resources, wake them, and drive their mouse, keyboard and terminal from a CLI, mainly on behalf of an AI agent.

## Language

**Device**:
A physical or virtual machine under management; the thing being controlled.
_Avoid_: machine, host, node, máy bị điều khiển

**Device Status**:
Whether a Device is reachable: Online (its Agent is connected), Offline (disconnected for more than 30 seconds) or Waking (a Wake was sent and the Device has up to 3 minutes to come Online). Screen lock and logged-in user are properties of an Online Device, not statuses.
_Avoid_: state, presence

**Agent**:
The software installed on a Device that reports its state and carries out Actions.
_Avoid_: client, daemon

**Server**:
The central service that keeps the Device registry and relays Actions between Controllers and Agents.
_Avoid_: hub, master

**Controller**:
Whoever issues Actions to a Device: a person or an AI agent using the CLI.
_Avoid_: client, máy điều khiển, AI agent (when it means the controlling side)

**Action**:
A single operation a Controller asks a Device to perform, such as a click, a keystroke or a terminal command.
_Avoid_: command, operation, thao tác

**Action Batch**:
An ordered list of Actions sent in one request and carried out by the Agent in sequence, returning a single Observation.
_Avoid_: script, macro, chain

**Observation**:
What a Device reports back after an Action or Action Batch so the Controller can see the result: nothing, one screenshot, a series of change-triggered frames, or a short clip.
_Avoid_: result, feedback, screenshot (when meaning the general concept)

**Mark**:
A numbered on-screen element (button, field, text) the Agent detects and labels in an Observation, so a Controller can target it by number or text instead of raw coordinates.
_Avoid_: element id, hotspot, target

**Wake Relay**:
An online Agent on the same LAN as an offline Device that sends the Wake-on-LAN packet on the Server's behalf.
_Avoid_: WoL proxy, gateway

**Enrollment Code**:
A single-use code created by an admin that lets a new Agent register its Device with the Server.
_Avoid_: invite, pairing code, install key

**Control Lock**:
Exclusive right held by one Controller at a time to send mouse and keyboard Actions to a Device; others may still observe, read metrics and use the terminal. It cannot be taken over by another Controller: it ends only when its holder releases it, sends no Action for 10 minutes, or stays disconnected for 60 seconds, or when an admin force-releases it.
_Avoid_: session lock, ownership

**Live View**:
A continuous stream of a Device's screen to a person in the Dashboard; the holder of the Control Lock can also send mouse and keyboard Actions through it. Unlike an Observation, it is not tied to any Action.
_Avoid_: screen share, remote desktop, xem màn hình

**Operator**:
A User who observes and controls Devices but cannot administer the Server.
_Avoid_: staff, normal user

**Admin**:
A User who, besides everything an Operator can do, manages Users, Enrollment Codes, Agent updates and Devices.
_Avoid_: administrator, superuser, quản trị viên
