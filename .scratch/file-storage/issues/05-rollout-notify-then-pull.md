# 05: Rollouts notify; the Agent pulls the build in the background

**What to build:** A rollout no longer pushes the build over the Agent's mTLS session. The Server→Agent `Update` message carries only version, target, size, sha256 and signature. On receiving it, the Agent starts a background task, at most one per version. The task downloads the build with ticket 04's routine, using the Server Key and its Device name, then installs it. Meanwhile the session keeps heartbeating and serving shells and Live View.
- **Transient failures** (network, 5xx) retry after a random 30–60 s.
- **Permanent failures** (bad signature, wrong target, no update key pinned) send `UpdateFailed` with the reason.

The platform install step takes a path to the verified file instead of the bytes. See the spec: Rollout message, Agent update flow, Platform install step, Concurrency, and user stories 12–16, 21 and 22.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] The `Update` message has version, target, size, sha256 and signature, and no build bytes.
- [ ] The frame limit for authenticated Device sessions returns to the normal session limit.
- [ ] The Agent downloads and installs off its session loop, with at most one task per version. After installing it reconnects or restarts as today.
- [ ] Retries:
  - transient errors retry after a random 30–60 s
  - permanent errors send `UpdateFailed` and show up as the Device's update error on the Dashboard
- [ ] The platform install step takes a file path. All adapters (Linux, macOS, Windows, fake) are updated.
- [ ] The server-side download path carries a `shortcut:` comment noting there is no cap on concurrent downloads and to add one when fleets grow.
- [ ] Testkit cases:
  - a rollout reaches fake Agents, which report the new version
  - a bad build is reported as an update error
  - a Device stays responsive (heartbeats and a command round-trip) while a large build downloads
- [ ] The Dashboard e2e rollout test still passes.
