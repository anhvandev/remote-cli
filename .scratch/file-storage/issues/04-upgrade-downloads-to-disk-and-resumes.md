# 04: `rcli upgrade` downloads to disk and resumes

**What to build:** A shared download routine fetches a build from the build lookup endpoint:
- It writes to `<data dir>/updates/<sha256>.part`, and on an interrupted download resumes from the file's length with `Range`.
- When the file is complete, it checks the size and sha256, then verifies the signature with the pinned update key.
- A file that fails a check is deleted and downloaded again from zero.

`rcli upgrade` switches to this routine, so it works on slow links and never holds the build in memory. Its user-facing behaviour and errors are unchanged. See the spec: `rcli upgrade` and Agent update flow (the download part), and user stories 17–20, 23 and 24.

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] The download routine:
  - streams to `<sha256>.part` and resumes with `Range` from an existing `.part`
  - checks size, sha256, then signature
  - deletes the file and restarts from zero on a mismatch
  - removes leftover files after success or a permanent failure
- [ ] `rcli upgrade` uses the routine. Its output and errors are unchanged: a bad signature still keeps the old binary.
- [ ] Testkit cases:
  - upgrade completes from a pre-existing partial `.part` file
  - a `.part` holding wrong bytes is discarded and the upgrade still succeeds
  - the existing upgrade tests still pass
