# 03: Agent builds stored in and streamed from storage

**What to build:** Agent builds move out of Postgres into the storage service from ticket 01. An upload streams into storage while the Server computes the sha256, then the signature is checked (ticket 02). A refused upload leaves neither a row nor an object. Every download of a build streams from storage with `Range` support and an `x-rcli-sha256` header:
- the build lookup used by `rcli upgrade` and the Agent
- `/download/{os}/{arch}` for the install one-liners

Until ticket 05, the rollout still sends the whole build over the mTLS session, read from storage. See the spec: Schema (agent_builds), Upload: Agent builds, Download endpoints, and user stories 1, 4, 6, 25, 27, 28 and 32.

**Blocked by:** 01, 02

**Status:** resolved

- [ ] A migration drops `agent_builds.build` and adds `sha256` and `size`.
- [ ] Publish:
  - streams the body into storage at `agent-builds/<sha256>`, keeping the 200 MiB limit
  - verifies the signature with the computed sha256
  - on any refusal (bad signature, no key, duplicate, error), deletes the object unless another row references it, and writes no row
- [ ] The build lookup and `/download/{os}/{arch}`:
  - stream from storage
  - send `Content-Length`, `x-rcli-version`, `x-rcli-signature` and `x-rcli-sha256`
  - answer `Range` with `206`
  - keep their access rules (Server Key, `?device=`, rollout/fleet selection) unchanged
- [ ] A corrupted build object is refused, not served.
- [ ] The rollout to Agents still works: the Server reads the build from storage for the existing `Update` message.
- [ ] The testkit rollout, upgrade and install-script tests pass against storage. New cases cover:
  - a range request on a build
  - the `x-rcli-sha256` header
  - a corrupted build object being refused
