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
Whoever issues Actions to a Device: a person or an AI agent, through the CLI or the Dashboard.
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

**Server Key**:
The secret every Agent and Controller must present to be allowed to connect to the Server; together with the Server's address it is all a new install needs. Replacing it locks out new installs but not Devices already registered.
_Avoid_: enrollment code, install key, token

**Server Address**:
The host and port Agents use to reach the Server, set by an Admin; together with the Server Key it is all a new install needs.
_Avoid_: hostname, server URL, endpoint

**Secret Store**:
The encrypted place on a Device where its Agent keeps the Device Password and the unlock password, readable only with root/Administrator rights on that Device.
_Avoid_: vault, keystore, credential store

**Device ID**:
The 9-digit number the Server gives a Device when it first registers, used to address it. It never changes by itself, only when deliberately changed.
_Avoid_: device uuid, machine id

**Device Password**:
The static secret of a Device, generated at install and viewable and changeable on the Device, that lets a Controller connect to that Device by its Device ID without a User account.
_Avoid_: access code, PIN, one-time password

**Device Settings**:
The rules set on the Device itself, not on the Server, that govern every Controller connecting to it: which Terminal User runs commands, whether root is never allowed, asked for or always allowed, and whether a person at the Device must accept each connection.
_Avoid_: permissions, policy, roles (when meaning what a Controller may do on a Device)

**Terminal User**:
The local account a Device runs terminal commands and shells as, by default the account that installed the Agent.
_Avoid_: run-as user, shell user

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
A User who, besides everything an Operator can do, manages Users, the Server Key, Agent updates and Devices.
_Avoid_: administrator, superuser, quản trị viên
