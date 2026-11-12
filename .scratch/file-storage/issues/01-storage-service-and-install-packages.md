# 01: Storage service; install packages stored and streamed there

**What to build:** A file storage service (SeaweedFS, S3-compatible) runs inside the compose network. Install packages (`.deb`, `.rpm`, `.dmg`) live in it instead of Postgres. An Admin publishes a package with `rcli package publish` and a person downloads it from the Connect guide exactly as before, but the bytes stream through the Server without being held whole in memory, and downloads resume with HTTP `Range`. This slice lays the storage foundation (config, object store module, test containers, docs, ADR) that later tickets reuse for Agent builds. See the spec: Solution, and Implementation Decisions under Storage service, Server configuration, Object store module, Schema (install_packages), Upload: install packages, Download endpoints, Docs and ADR.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [ ] Compose has a `storage` service:
  - pinned SeaweedFS image, S3 gateway
  - named volume `filedata`
  - no host port published
- [ ] The Server requires `RCLI_S3_ENDPOINT`, `RCLI_S3_BUCKET`, `RCLI_S3_ACCESS_KEY` and `RCLI_S3_SECRET_KEY`. Startup fails with a message naming each missing variable.
- [ ] The Server creates the bucket if it is missing.
- [ ] The object store module:
  - streams uploads in and returns their `{sha256, size}`
  - stores objects at the content addresses `packages/<sha256>`
  - streams reads, with an optional byte range
  - refuses to serve a full object whose bytes don't match the recorded sha256
  - leaves no object behind when an upload fails
- [ ] A migration drops `install_packages.package` and adds `sha256` and `size`.
- [ ] Package publish:
  - streams into storage
  - still replaces the package for that kind + arch
  - deletes the replaced object when nothing else references it
- [ ] Package download:
  - streams from storage
  - sends `Content-Length`
  - answers `Range: bytes=N-` / `N-M` with `206` and `Content-Range`
- [ ] `TestSystem` starts a SeaweedFS container via testcontainers next to Postgres and passes the `RCLI_S3_*` settings to the Server. The dev stack binary does the same.
- [ ] Testkit tests cover:
  - package publish then download returns identical bytes
  - a range request returns the right slice with `206`
  - replacing a package serves the new bytes
  - a corrupted object (bytes changed directly in storage) is refused, not served
- [ ] Deploy docs (compose header comments and the deploy guide):
  - list the `storage` service and the `RCLI_S3_*` variables
  - state that a backup is `pg_dump` plus the `filedata` volume
- [ ] A short ADR records three decisions:
  - builds and packages live in an internal S3-compatible store; the Server streams them and never exposes the store
  - the update signature covers the sha256
  - rollouts are notify-then-pull over HTTPS
