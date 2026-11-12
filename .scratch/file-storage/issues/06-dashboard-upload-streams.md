# 06: The Dashboard's build upload streams through a route handler

**What to build:** The Admin's Agent build upload on the Updates page no longer goes through a Next.js server action, which reads the whole file into memory. The form posts to a Dashboard route handler that checks the Admin's session like the other Dashboard routes. The handler streams the request body to the Server's publish endpoint through the existing node:http forward helper (the one the web terminal uses), and the form shows the Server's success or error as today. See the spec: Upload: Dashboard, and user story 3.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] The upload form posts to a route handler. The server-action upload is removed.
- [ ] The handler:
  - rejects requests without a valid Admin session
  - streams the body to the Server without buffering it
  - relays the Server's status and error message to the form
- [ ] The raised server-action `bodySizeLimit` is removed from the Next.js config.
- [ ] The existing e2e test "admin uploads a signed Agent build and rolls it out to selected Devices" passes. The form still shows `Đã tải lên <version>` on success and the Server's message on failure.
