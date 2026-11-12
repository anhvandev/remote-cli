# Agent Builds are not signed; the Server's sha256 is the only check

Publishing an update took a signing key (`rcli rollout keygen`), a public key configured on the Server and pinned at every install, a signature typed into the upload form, and a separate rollout step. That was too slow for a small team, so a User now uploads only the binary with its version and target. The Server computes the sha256 on upload, and a Device or Controller fetching the Latest Build checks the file against that hash. Against an on-path attacker we rely on the API Address being `https://`; a MAC under the Server Key would add nothing, because the build lookup already sends that secret in a request header. There is no per-Device rollout: a machine updates when someone runs `rcli update` on it, or when its Agent is below the Minimum Version and the Server tells it to fetch the Latest Build.

## Consequences

- We accept that whoever controls the Server can ship any binary to every Device. A signature held offline would have stopped that.
- Agents on v0.2.3 or older accept only signed builds, so the first version without signing is shipped one last time through the old signed rollout.
- No canary and no rollback by version: `rcli update` only moves up to the Latest Build. To roll back, delete the bad build and upload the old code under a higher version.
