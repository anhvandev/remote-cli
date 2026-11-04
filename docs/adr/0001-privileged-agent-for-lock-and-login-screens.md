# Agent runs with root/SYSTEM privileges to control lock and login screens

Devices must be controllable unattended, including at the lock screen and the login screen (for example after a Wake-on-LAN boot), on Windows, macOS (where feasible), Linux X11 and Linux Wayland. Ordinary user-level processes cannot see or drive those screens, so the Agent runs a privileged component: a SYSTEM service on Windows, a root daemon plus a login-window agent on macOS, and root on Linux for uinput input. We accept the larger blast radius and contain it with per-Device mTLS credentials, admin-only `--as root` terminal access, and an audit log of every Action. Unlock passwords live encrypted on the Server and are typed by the Agent, so no Controller ever sees them.

## Considered Options

- User-level Agent with auto-login required: simpler and safer, but Devices that reboot or lock become unreachable. Rejected.

## Consequences

- macOS login window is view-only. A probe on macOS 15.8.1 (`prototype/lockscreen-probe/`, 2026-11-04) showed that a root LoginWindow agent can capture the screen once Screen Recording is granted, but every CGEvent key-injection variant (including RustDesk's) failed to reach the password field. Lock screen capture and input both work. Macs therefore need auto-login (or a person logging in once) after a reboot, and the Dashboard shows when a Device is at the login window.
