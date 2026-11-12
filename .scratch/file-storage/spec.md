# File storage: Agent builds and install packages out of Postgres

**Status:** ready-for-agent

## Problem Statement

An Admin who uploads an Agent build (up to 200 MiB) or an install package (`.deb`, `.rpm`, `.dmg`) has it stored inside Postgres as a BYTEA column. The database, and every `pg_dump` backup of it, grows by tens of megabytes per build. Every component that touches a build holds the whole file in RAM:
- the Dashboard's upload server action
- the Server when it stores or serves a build
- the Agent when it receives one

A rollout pushes the entire build to each Device inside a single frame on the Agent's mTLS session. That session also carries heartbeats, shells and Live View signalling, so a large build on a slow link blocks them all and can make the Server think the Device went offline. The Agent installs inline in its main loop and stops heartbeating while it does. An interrupted transfer starts over from zero.

The Server stores whatever signature the Admin pastes without checking it. A mistake, such as pasting the Windows signature next to the Linux build, only shows up after a rollout, when every Device reports a failure. Because a version + target cannot be uploaded twice, the Admin then has to bump the version.

## Solution

A dedicated file storage service, S3-compatible SeaweedFS, runs inside the compose network and holds all Agent builds and install packages. Postgres keeps only the metadata. Files are streamed end to end and never held whole in memory:
- **Upload:** browser → Dashboard → Server → storage.
- **Download:** storage → Server → Agent, `rcli upgrade`, the install one-liners or a package download.

The Server checks an Agent build's signature against the update public key at upload time, so a bad signature is refused immediately with a clear error.

A rollout no longer pushes the build over the mTLS session. The Server only tells the Device which build to fetch: version, target, size, sha256 and signature. The Agent downloads it over HTTPS in the background, writing to disk. If the connection drops, the Agent resumes the download where it stopped, and it verifies the sha256 and the signature before installing.

The update signature now covers the build's sha256 instead of the whole file, so it can be checked without the file in memory.

## User Stories

1. As an Admin, I want uploaded Agent builds stored outside the database, so that the database and its backups stay small.
2. As an Admin, I want uploaded install packages (`.deb`, `.rpm`, `.dmg`) stored outside the database, so that the database and its backups stay small.
3. As an Admin, I want to upload a 200 MiB Agent build from the Dashboard without the Dashboard holding it in memory, so that a large upload cannot exhaust the Dashboard container's RAM.
4. As an Admin, I want the Server to stream an upload into storage while it receives it, so that the Server's memory use does not grow with the build size.
5. As an Admin, I want an upload whose signature does not match the update public key refused immediately, so that I find a wrong signature before rolling it out.
6. As an Admin, I want a refused upload to leave nothing behind, neither the metadata row nor the stored object, so that I can upload the same version + target again with the right signature.
7. As an Admin, I want uploads refused with a clear error while the Server has no update public key configured, so that I don't publish a build no Device can verify.
8. As an Admin, I want the error for a bad signature to say that the signature does not match this version, target and file, so that I know which field to fix.
9. As an Admin, I want uploading an existing version + target to keep failing as it does today, so that a published build never changes under the Devices that already have it.
10. As an Admin, I want to sign a build with `rcli rollout publish` exactly as before, so that the change of signature format is invisible to me when I use the CLI.
11. As an Admin who signs builds outside rcli, I want the signed message format documented (`rcli-agent-build\n{version}\n{target}\n{sha256 hex}`), so that I can produce a signature with standard tools.
12. As an Admin, I want a rollout to reach a Device without blocking its heartbeats, shells or Live View, so that the Device does not appear offline while it updates.
13. As an Admin, I want "Cập nhật tất cả" across the fleet to cost the Server roughly constant memory, so that a fleet-wide rollout does not run the Server out of RAM.
14. As an Admin, I want the Dashboard's rollout progress and the per-Device update errors to work as they do today, so that I can follow a rollout the same way.
15. As an Admin, I want a Device whose download failed to retry on its own after a short random delay, so that a transient network error does not need my attention.
16. As an Admin, I want a Device whose download or verification failed for good to report the reason, as `UpdateFailed` does today, so that I see it on the Dashboard.
17. As a Device on a flaky network, I want an interrupted build download to resume from the bytes I already have, so that I don't restart a 55 MB download from zero.
18. As a Device, I want a partial download kept under a name derived from the build's sha256, so that a resume can never mix bytes from two different builds.
19. As a Device, I want to verify the downloaded file's sha256 and the update signature before installing, so that a corrupted or tampered build is never installed.
20. As a Device, I want a partial or complete file that fails verification deleted and downloaded again from the start, so that one bad file does not leave me stuck.
21. As a Device, I want the download and install to run off my main session loop, so that I keep heartbeating while I update.
22. As a Device, I want to fetch the build with the same credentials `rcli upgrade` uses (the Server Key header and my Device name), so that the Server serves only the build rolled out to me.
23. As a Controller running `rcli upgrade`, I want the build streamed to disk and resumable, so that upgrading works on slow links and small machines.
24. As a Controller running `rcli upgrade`, I want the same sha256 and signature checks as the Agent, so that a bad build is rejected and my old binary kept.
25. As a person installing with `install.sh` or `install.ps1`, I want the one-liner to download the fleet's build from storage exactly as before, so that the install command does not change.
26. As a person following the Connect guide, I want package download links (`.deb`, `.rpm`, `.dmg`) to keep working, so that I can install from a package.
27. As any downloader, I want the Server to answer `Range` requests with `206 Partial Content`, so that resuming works over plain HTTP.
28. As any downloader, I want each response to carry the file's size and sha256, so that I can check what I received.
29. As an operator of the Server, I want the Server to refuse to start when the storage settings (`RCLI_S3_ENDPOINT`, `RCLI_S3_BUCKET`, `RCLI_S3_ACCESS_KEY`, `RCLI_S3_SECRET_KEY`) are missing, naming the missing ones, so that a misconfigured deploy fails loudly instead of at the first upload.
30. As an operator, I want the storage service reachable only inside the compose network, so that files are served only through the Server's access checks (ADR 0006: only the Dashboard is public).
31. As an operator, I want storage data in its own named volume (`filedata`), so that it survives container rebuilds.
32. As an operator, I want the Server to detect an object whose bytes no longer match the sha256 recorded in Postgres and fail that download, so that storage corruption is noticed and never served as valid.
33. As an operator, I want the deploy documentation to say that a backup is `pg_dump` plus the `filedata` volume, so that I don't assume the database dump still contains the builds.
34. As an operator upgrading an existing deployment, I want the migration to drop the BYTEA columns without moving data, so that the upgrade is a plain restart (the only deployment has no stored builds).
35. As a developer, I want the test system to start a real SeaweedFS container next to Postgres, so that tests exercise the production storage path.
36. As a developer, I want the dev stack used by the Dashboard's e2e tests to include storage, so that the upload UI is tested against the real path.

## Implementation Decisions

- **Storage service.**
  - A new compose service `storage` runs SeaweedFS with its S3 gateway, from a pinned `chrislusf/seaweedfs` image tag, with data on the named volume `filedata`. It publishes no host port.
  - It holds a single bucket, `rcli`. The Server creates it if missing at startup.
- **Server configuration.**
  - The Server reads `RCLI_S3_ENDPOINT`, `RCLI_S3_BUCKET`, `RCLI_S3_ACCESS_KEY` and `RCLI_S3_SECRET_KEY`.
  - Each is required. Startup fails with a message naming every missing variable.
  - Compose passes them from `.env`, like the other `RCLI_*` settings.
- **Object store module (Server).** A small module with a deep interface:
  - **put:** stream bytes in and get back `{sha256, size}`. It writes under a temporary key, computing the sha256 while streaming, then stores the object at its content address. It removes the temporary object on any failure.
  - **get:** take a content address and an optional byte range, and return a byte stream. It checks the bytes against the recorded sha256 when serving a full object.
  - **delete:** remove an object.
  - Object keys are content addressed: `agent-builds/<sha256>` and `packages/<sha256>`. The S3 client is the module's only adapter; no other backend exists.
- **Schema.**
  - **`agent_builds`:** drop `build`, add `sha256` and `size`. `version`, `target`, `signature`, the upload fields and `rolled_out_at` are unchanged.
  - **`install_packages`:** drop `package`, add `sha256` and `size`.
  - No data migration: the only deployment has zero rows in both tables.
- **Signature format (one-way door for signatures; a two-way door for code).**
  - The update signature is Ed25519 over `rcli-agent-build\n{version}\n{target}\n{sha256 lowercase hex}`.
  - `sign` and `verify` in the protocol crate take the sha256 instead of the build bytes.
  - `rcli rollout publish` hashes the file while streaming and signs the digest.
  - Signatures made in the old format stop verifying. None are in use: no builds are uploaded and no Device has pinned an update key yet.
- **Upload: Agent builds.**
  - The publish endpoint keeps its path, query and 200 MiB limit, but streams the request body into the object store instead of buffering it.
  - After the stream ends, the Server verifies the signature against the configured update public key using the computed sha256.
  - On a bad signature, a missing public key, an existing version + target or any error, it deletes the stored object (unless another row references the same sha256) and writes no row.
  - Distinct errors:
    - no update key configured
    - signature does not match
    - build already exists (unchanged)
- **Upload: install packages.** The package publish endpoint streams into the object store the same way. Its existing semantics, replacing the package for that kind + arch, are unchanged. The replaced object is deleted when no other row references it. There is no signature.
- **Upload: Dashboard.**
  - The Admin's build upload no longer goes through a server action.
  - The form posts to a Dashboard route handler that authenticates the Admin's session as the other Dashboard routes do. It forwards the request body as a stream to the Server's publish endpoint, through the existing node:http forward helper (the one the web terminal uses), and returns the Server's result for the form to show.
  - The Next.js server-action body size limit raised for uploads is no longer needed.
- **Download endpoints.**
  - All four routes stream from the object store:
    - the Device/Controller build lookup used by `rcli upgrade` and the Agent
    - `/download/{os}/{arch}`
    - `/download/package/{kind}/{arch}`
    - the Dashboard-forwarded equivalents
  - They support a single `Range: bytes=N-` (and `N-M`) with `206 Partial Content` and `Content-Range`, and send `Content-Length`.
  - Build responses keep the `x-rcli-version` and `x-rcli-signature` headers, and add `x-rcli-sha256`.
  - Access rules (Server Key header, `?device=`, rollout and fleet-build selection) are unchanged.
- **Rollout message.**
  - The Server→Agent `Update` message carries version, target, size, sha256 and signature, and no build bytes.
  - The frame limit for authenticated Device sessions returns to the normal session limit; the build-sized limit is removed.
  - Offering is unchanged: once per version per session, and on every rollout change.
- **Agent update flow.**
  - On `Update`, the Agent starts a background task, at most one per version, and the session loop continues.
  - The task resolves the API Address and downloads the build from the build lookup endpoint with the Server Key and its Device name.
  - It appends to `<Agent data dir>/updates/<sha256>.part`, resuming from the file's current length with `Range`.
  - When complete, it checks the size and sha256, then verifies the signature with the pinned update key. A mismatch deletes the file and retries from zero.
  - It then hands the file to the platform's install step and the session reconnects as today.
  - Transient failures (network, 5xx) retry after a random 30–60 s delay. Permanent failures (bad signature, wrong target, no update key pinned) send `UpdateFailed` with the reason.
  - The `.part` and finished files are removed after install or on permanent failure.
- **Platform install step.** It takes a path to the verified file instead of the bytes, so no adapter loads the build into memory.
- **`rcli upgrade`.**
  - It uses the same download routine as the Agent: stream to `<sha256>.part`, resume with `Range`, then check the sha256 and the signature.
  - Its user-facing behaviour, output and errors are otherwise unchanged.
- **Concurrency.** There is no server-side cap on simultaneous downloads. The cap site carries a `shortcut:` comment saying to add a limit when fleets grow.
- **Install one-liners.** `install.sh` and `install.ps1` are unchanged. They download without signature verification and trust HTTPS, as today.
- **Docs.**
  - The deploy documentation (compose header comments and the deploy guide) lists the new `RCLI_S3_*` variables and the `storage` service.
  - It states that a backup is `pg_dump` plus the `filedata` volume.
- **ADR.** Record a short ADR covering three decisions:
  - Agent builds and packages live in an internal S3-compatible store, never exposed; the Server streams them.
  - The update signature covers the sha256.
  - Rollouts are notify-then-pull over HTTPS.

## Testing Decisions

- **What makes a good test.**
  - Drive the system from the outside, through the `rcli` CLI, the HTTP API and the Agent's observable behaviour, and assert on what a user or Device sees: status codes, error messages, the installed version, rollout progress and update errors.
  - Never assert on S3 keys, SQL rows or internal module calls, except a direct check that the database dump no longer holds file bytes, if cheap.
- **Seam 1: `TestSystem` (crates/testkit).**
  - It starts a SeaweedFS container next to its Postgres container via testcontainers, and passes the `RCLI_S3_*` settings to the in-process Server.
  - Prior art: the rollout, upgrade and install-script test files, and the package publish tests. Extend these rather than adding new files where the behaviour already has a home.
  - Cover:
    - an upload with a correct signature is accepted
    - a wrong signature, a missing update key and a duplicate version + target are refused, and the refused upload can be retried
    - a rollout reaches fake Agents and they report the new version
    - a bad build is reported as an update error
    - `rcli upgrade` installs the rolled-out build and keeps the old binary on a bad signature
    - `Range` requests return `206` with the right bytes
    - an Agent or `rcli upgrade` with an existing partial `.part` file completes from it
    - a corrupted object (bytes changed in storage) is refused, not served
    - the install-script download and package download still work
    - a Device session stays responsive (heartbeats/commands) while a large build downloads
  - The tests that use `sign`/`verify` switch to the sha256-based signature.
- **Seam 2: Dashboard Playwright e2e.**
  - The existing "admin uploads a signed Agent build and rolls it out to selected Devices" test keeps passing through the new streaming route.
  - A new case shows the error when the pasted signature is wrong.
  - The dev stack binary (rcli-devstack) starts storage the same way `TestSystem` does and signs its fake build in the new format.
- **No unit tests** for the S3 client or object store module. They are covered through Seam 1. The existing protocol unit test for signature binding is updated to the new format.

## Out of Scope

- A server-side limit on concurrent build downloads (left as a `shortcut:`).
- Signature verification in `install.sh` / `install.ps1`.
- Signing install packages.
- Presigned URLs or exposing the storage service publicly.
- An automated backup script for `filedata`.
- Deleting or expiring old Agent builds (no delete feature exists today).
- Signing builds from the Dashboard (the private key stays off the Server).
- Moving any other data (history, audit) out of Postgres.
- Using an external S3 provider (R2, AWS). Pointing `RCLI_S3_ENDPOINT` at one should work, but is not tested.

## Further Notes

- The two signatures produced on 2026-11-12 for the v0.1.0 builds (`rcli-linux-x86_64`, `rcli.exe`) use the old format. They must be regenerated in the new format after this ships.
- The update public key (`38f2b28d…19e5`) is not yet configured on the production Server, and the one Device (`hoanghoi`) was installed without one. It must run `rcli install` again to pin the key before it can accept any rollout. This is unrelated to this spec but blocks using it in production.
- MinIO was considered and rejected: its community edition is in maintenance mode.
