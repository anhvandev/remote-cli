# Every User may do everything; there are no roles

The Server had two roles: an Operator controlled Devices and an Admin also administered the Server. In practice the team is small, everyone who signs in is trusted with the whole Server, and the split only hid tools such as deleting Devices behind an Admin page. We drop roles: every User manages Users, the Server Key, settings, Agent Builds and Devices. What a Controller may do *on* a Device is still set by that Device's settings (ADR 0004). This supersedes the Admin/Operator part of ADR 0004.

## Consequences

- A User cannot delete themselves or the last User, so the Server never ends up with nobody able to sign in.
- Any User's first `https://` Dashboard visit can record the API Address (ADR 0006), not only an Admin's.
