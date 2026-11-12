# Agent builds and install packages live in an internal object store

Agent builds (up to 200 MiB) and install packages were BYTEA columns in Postgres: every `pg_dump` grew by tens of megabytes per build, and every component held whole files in RAM. A rollout pushed the whole build inside one frame on the Agent's mTLS session, blocking heartbeats, shells and Live View. Three decisions follow:

- **Files live in an internal S3-compatible store.** A SeaweedFS service (`storage`, volume `filedata`) runs inside the compose network with no host port. Postgres keeps only metadata (sha256, size). Objects are content addressed (`packages/<sha256>`, `agent-builds/<sha256>`). The Server streams files in and out, checks a full read against the recorded sha256, and serves `Range` requests. The store is never exposed: files are served only through the Server's access checks (ADR 0006: only the Dashboard and the Agent port are public).
- **The update signature covers the sha256.** It is Ed25519 over `rcli-agent-build\n{version}\n{target}\n{sha256 hex}`, so the Server can check it at upload time and a Device before installing without holding the file in memory.
- **Rollouts are notify-then-pull over HTTPS.** The Server tells a Device which build to fetch (version, target, size, sha256, signature); the Agent downloads it in the background with `Range` resume and verifies it before installing.

## Considered Options

- Keep files in Postgres: simplest, but the backup and memory problems are the reason for this change.
- MinIO: rejected, its community edition is in maintenance mode.
- Presigned URLs straight to the store: rejected, it would expose the store and bypass the Server's access rules.

## Consequences

- A backup is `pg_dump` plus the `filedata` volume.
- The Server needs `RCLI_S3_ENDPOINT`, `RCLI_S3_BUCKET`, `RCLI_S3_ACCESS_KEY` and `RCLI_S3_SECRET_KEY` and refuses to start without them.
- Signatures in the old whole-file format stop verifying; none were in use.
