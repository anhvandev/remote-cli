# 02: Update signature covers the sha256; the Server refuses bad signatures at upload

**What to build:** The Agent build signature becomes Ed25519 over `rcli-agent-build\n{version}\n{target}\n{sha256 lowercase hex}` instead of over the whole build, so it can be checked without the file in memory. At upload the Server checks the signature against the configured update public key. A wrong signature or a missing public key is refused immediately with a clear error, and nothing is stored, so the Admin can retry the same version + target. Builds are still stored in Postgres in this slice. See the spec: Signature format, Upload: Agent builds (verification and errors), and user stories 5–11.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [ ] Protocol `sign`/`verify` take the sha256 instead of the build bytes. The existing signature-binding unit test covers the new format (key, version, target and digest each bind).
- [ ] `rcli rollout publish` hashes the file and signs the digest. Its usage and output are unchanged.
- [ ] The Server verifies an uploaded build's signature with the update public key. The distinct errors are:
  - no update key configured
  - signature does not match this version, target and file
  - build already exists (unchanged)
- [ ] A refused upload writes no row, and the same version + target can then be uploaded with a correct signature.
- [ ] The Agent and `rcli upgrade` verify the new format. A bad signature is still rejected: the Agent reports `UpdateFailed` and `rcli upgrade` keeps the old binary.
- [ ] The dev stack signs its fake build in the new format.
- [ ] Testkit rollout and upgrade tests use the new format. New cases cover a wrong signature, a missing update key, and a refused upload followed by a successful retry.
- [ ] A Dashboard e2e case shows the error when the Admin pastes a wrong signature.
