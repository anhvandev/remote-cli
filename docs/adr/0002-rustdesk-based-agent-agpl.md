# Agent in Rust reusing RustDesk capture/input code; project licensed AGPL-3.0

Screen capture and input injection across Windows, macOS, X11 and Wayland, including lock and login screens, is the hardest and most platform-specific part of the system. RustDesk already solves most of it, so the Agent is written in Rust and reuses RustDesk's capture/input code. The Agent is our own crate: it depends on RustDesk's `scrap` and `enigo` through a git dependency pinned to a commit, so it keeps receiving upstream fixes. The lock/login-screen orchestration (Windows service, macOS LoginWindow agent, Linux seat0 session discovery) is ported into the Agent and rewritten more compactly. We do not fork the whole RustDesk app. The Server, CLI, MCP layer and Dashboard are our own code. RustDesk is AGPL-3.0, so the whole project is AGPL-3.0, which fits our open-source plus internal-use goal.

## Considered Options

- Write everything from scratch (Go): full control, but months of per-OS capture/input work.
- Fork MeshCentral (Apache-2.0): ready Server and Dashboard, but deep AI-oriented changes (Observation, Marks, metrics history, Wayland) would fight a Node.js + C agent codebase.
