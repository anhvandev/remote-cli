# 01: Walking skeleton: enroll a Device and list it

**What to build:** Stand up the thinnest end-to-end path through every part of the system. An admin creates an Enrollment Code with the CLI. An Agent running on the FakePlatform enrolls with it and keeps a persistent mTLS connection. `rcli devices` then lists the Device with its Device Status. This ticket also establishes the in-process test harness (real Server on Postgres in a test container, real Agent with FakePlatform, test client on the Controller API, injectable clock) and the Docker Compose stack (Server + Postgres). Every later ticket builds on both. Use a bootstrap admin token from Server config until users exist (ticket 02).

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Rust workspace with Server, Agent, CLI and a shared protocol crate (protobuf messages); AGPL-3.0 license file
- [ ] Admin creates a single-use Enrollment Code; an Agent enrolls with it and receives its own mTLS client certificate; reusing the code fails
- [ ] Agent holds a persistent outbound TLS connection authenticated by its certificate
- [ ] `rcli devices` (human and JSON output) shows the Device as Online while connected
- [ ] Device becomes Offline 30 seconds after its connection drops (verified with the injectable clock)
- [ ] The Agent talks to the OS only through the Platform port; the FakePlatform implements it
- [ ] Test harness runs the whole system in-process; at least one test covers enrollment → Online → Offline through the Controller API
- [ ] Docker Compose brings up Server + Postgres
