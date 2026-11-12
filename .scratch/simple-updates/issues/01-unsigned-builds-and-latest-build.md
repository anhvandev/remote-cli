# 01: Unsigned Agent Builds served as the Latest Build

**What to build:** A User uploads an Agent Build with only the file, a semver version and a target, from the Updates page with no signature field. The Server computes the sha256. The build lookup used by `rcli upgrade` and the `/download` route for new installs always serve the Latest Build for the target (highest semver). The client checks the downloaded file only against the Server's sha256. The whole signing and rollout system goes away (ADR 0008): `rcli rollout` (keygen, publish, roll out), the update public key config, the update key pinned at install and its endpoint, the per-Device rollout state, the fleet build, and the rollout UI. The Agent's existing update task still installs a build when it receives an `AgentUpdate`, and ignores any signature in it, so a new Agent keeps working with an old Server during the transition.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [ ] Uploading a build with no signature succeeds and the Server stores its sha256 and size
- [ ] A non-semver version is refused; an existing (version, target) is refused with "delete the old one first"
- [ ] The build lookup and `/download` serve the highest-semver Agent Build for the target (a pre-release ranks below its release)
- [ ] A download whose sha256 does not match is discarded and the old binary is left untouched; resuming a partial download still works
- [ ] The Updates page upload form has file, version and target only, prefilled from a name like `rcli-0.3.0-linux-x86_64`
- [ ] `rcli rollout`, the update-key endpoint, the update key in registration and the machine config, the rollout API, the rollout columns and the rollout UI are gone; the Device JSON no longer has the `update` object
- [ ] An Agent receiving an `AgentUpdate` that carries a signature still installs the build (signature ignored)
- [ ] The testkit rollout/upgrade tests are rewritten for the new behaviour (no signatures anywhere)
