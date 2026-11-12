# 03: List and delete Agent Builds

**What to build:** The Updates page lists every Agent Build grouped by version, newest first. Each target shows its size, short sha256, uploader and upload time, and the Latest Build per target is marked "Latest". A User can download any build, delete one build (version + target), or delete a whole version, each after a confirmation. Deleting removes the stored file once nothing references it. Deleting the Latest Build makes the next-highest version the Latest Build. The CLI can list, upload and delete builds too, so a release script can publish without the Dashboard.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] A list API returns every Agent Build with version, target, size, sha256, uploader, upload time and whether it is the Latest Build for its target
- [ ] Delete one (version, target) and delete a whole version both work; the stored object is removed once unreferenced
- [ ] After the Latest Build is deleted, the build lookup serves the next-highest version
- [ ] The Updates page shows the grouped list with Latest marks, download, and delete with confirmation
- [ ] The CLI lists, uploads (file + version + target) and deletes Agent Builds
- [ ] A Playwright e2e uploads a build, sees it marked Latest, and deletes it
