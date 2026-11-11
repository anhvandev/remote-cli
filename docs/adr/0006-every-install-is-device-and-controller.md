# Every install is both a Device and a Controller

There used to be two setups: `rcli agent install` (root/SYSTEM) made a machine a Device, and `rcli setup` (any user) only let a machine control others, with a different address. Users found two commands for "install rcli" confusing. Like TeamViewer and RustDesk, there is now one command, `rcli install --server <Server Address> --key <Server Key>`: it installs the Agent, registers the Device, prints its Device ID and Device Password, and writes the Controller configuration, so `rcli connect` and `rcli mcp` work right after. The Server hands the install its API Address at registration and on every Agent connection, so the user still types only the Server Address and the Server Key. To make the API Address discoverable, the Controller API is served through the Dashboard's public URL (the Dashboard forwards `/api/v1/*`), and the Server records that URL from an Admin's first `https://` Dashboard request whose host is not localhost or a private IP. An Admin can correct it in Server Settings. The Server Address, Server Key and API Address are stored machine-wide, read-only for every local user. Remembered Device Passwords stay per user.

## Considered Options

- Keep two commands with clearer docs: a controlling machine is never controllable, and it needs no admin rights. Rejected: the setup confusion is the problem being solved.
- One command with `--controller-only`: kept the safety of the above, but brings back two paths. Deferred until someone needs it.
- Controllers call the API through the Agent port with the Device's mTLS certificate, so there is only one address: cleanest, but changes Controller authentication and the reverse proxy setup. Rejected for now.

## Consequences

- Every machine that controls others, including one running an AI agent, can itself be controlled by anyone with its Device Password. Device Settings (accept each connection, root access) are what protect it.
- Only the Dashboard port and the Agent port face the internet; the Controller API port stays internal. Streams (shell, `lock/watch`) go through the Dashboard.
- Until an Admin has opened the Dashboard over `https://`, installs succeed as Devices but Controller commands report that the Server has no API Address yet.
- Installing needs sudo/UAC even when the user only wants to control others.
- `rcli setup`, `rcli agent install` and `rcli agent uninstall` are removed with no alias; `rcli uninstall` undoes `rcli install` but keeps each user's remembered Device Passwords. No Device was installed yet, so nothing is migrated.
- Audit still names a Controller connected by Device Password by machine name and IP (ADR 0003), not by its Device ID.
