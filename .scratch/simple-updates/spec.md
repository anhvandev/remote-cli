# Simple updates: upload a build, `rcli update`, no roles

**Status:** resolved

## Problem Statement

Shipping a new rcli version to the team's machines takes too many steps and too much ceremony:
- A User must create a signing key (`rcli rollout keygen`), configure its public key on the Server, and have it pinned at every install.
- Every upload needs a signature typed into the form.
- Each Agent Build must then be rolled out, to some Devices or to all, as a separate step.

Rollouts that fail leave Devices stuck in a failed-update state that lingers in the Device list. There is no plain "update this machine now" command. Telling which machines run an old version means reading versions row by row.

The split between Admin and Operator hides everyday tools, such as deleting a dead Device, behind an Admin-only page. The team is small and everyone who signs in is trusted with the whole Server.

## Solution

- **Upload:** a User uploads an Agent Build by choosing the file, a semver version and a target. Both fields are prefilled from the file name. There is no key, no signature and no rollout. The Server computes the sha256 itself.
- **Updates page:** lists Agent Builds grouped by version. Any file, or a whole version, can be deleted.
- **Latest Build:** for each target, the Agent Build with the highest version is the Latest Build. New installs download it.
- **`rcli update`:** on any machine, downloads the Latest Build, checks it against the Server's sha256, replaces the binary and restarts the Agent. It only ever moves up.
- **Minimum Version:** a User sets it in Server settings. Devices below it get an "update needed" badge, and Controllers below it print a warning. Neither is blocked. An Agent below the Minimum Version updates itself to the Latest Build and retries every hour if that fails. Raising the Minimum Version is therefore how a User updates the whole fleet.
- **No roles:** every User may do everything. The Device admin tools (rename, Device ID, Device Password, merge, revoke, delete) move into a per-row menu on the Devices page.

See ADR 0008 (unsigned Agent Builds) and ADR 0009 (no User roles).

## User Stories

### Uploading and managing Agent Builds

1. As a User, I want to upload an Agent Build by choosing only the file, a version and a target, so that publishing a new version takes seconds.
2. As a User, I want the version and target prefilled from a file named like `rcli-0.3.0-linux-x86_64`, so that I usually only press Upload.
3. As a User, I want the upload refused with a clear message when the version is not semver (`X.Y.Z` with an optional pre-release suffix), so that "latest" and "below minimum" are always well defined.
4. As a User, I want an upload of a version + target that already exists to be refused with a message telling me to delete the old one first, so that I never overwrite a build by accident.
5. As a User, I want the Server to compute and store the sha256 and size of every upload, so that I never handle hashes or keys.
6. As a User, I want the Updates page to list Agent Builds grouped by version, newest first, with each target's size, sha256 (shortened), uploader and upload time, so that I can see what the Server holds.
7. As a User, I want the version holding the Latest Build for a target marked "Latest", so that I know what `rcli update` installs.
8. As a User, I want to download any stored Agent Build from the Updates page, so that I can test or hand it out manually.
9. As a User, I want to delete one Agent Build (version + target) after a confirmation, so that I can remove a broken or mislabelled file.
10. As a User, I want to delete a whole version after a confirmation, so that I can withdraw a bad release in one action.
11. As a User, I want deleting the Latest Build to make the next-highest version the Latest Build, so that withdrawing a bad release is enough to roll back the target of every future update.
12. As a User, I want the stored file removed from object storage when its Agent Build is deleted, so that deleted builds do not use space.
13. As a User, I want Devices running a deleted version to keep running normally, so that deleting a build is never destructive to a machine.
14. As a User, I want to upload an Agent Build from the CLI with only the file, version and target, so that a release script can publish without the Dashboard.

### Updating a machine

15. As a person at a machine, I want to run `rcli update` (with sudo on an installed Device), so that the machine moves to the Latest Build for its target.
16. As a person running `rcli update`, I want to see the current version, the Latest Build's version, download progress, the hash check and the restart, so that I know what happened.
17. As a person running `rcli update` on a machine already at the Latest Build, I want "already the latest version (X)" and exit code 0, so that scripts can run it unconditionally.
18. As a person running `rcli update` on a machine newer than the Latest Build, I want "running X, newer than the Server's latest (Y)" and no change, so that update never downgrades.
19. As a person running `rcli update`, I want a download whose sha256 does not match the Server's to be discarded with an error and the old binary left untouched, so that a corrupt download never breaks the machine.
20. As a person running `rcli update` when the Server has no Agent Build for this target, I want a clear "no build for <target>" error, so that I know what to upload.
21. As a person running `rcli update` over a flaky network, I want an interrupted download to resume, so that large builds still arrive.
22. As someone with an old script, I want `rcli upgrade` to keep working as a hidden alias of `rcli update`, so that nothing I wrote breaks.
23. As a User installing a new machine, I want the install to download the Latest Build, so that new machines start current.

### Minimum Version

24. As a User, I want to set a Minimum Version in Server settings, or leave it empty for none, so that I can say which versions are too old.
25. As a User, I want the Minimum Version saved without any check against the stored Agent Builds, so that it is a recommendation I control.
26. As a User, I want Devices whose Agent is below the Minimum Version to show an "update needed · X < Y" badge on the Devices page, so that stragglers stand out.
27. As a User, I want `rcli devices` to show the same "update needed" state, so that the CLI tells me what the Dashboard does.
28. As a User, I want a Device below the Minimum Version to keep working normally (control, shell, Live View), so that the Minimum Version never locks anyone out.
29. As a User, I want an Agent below the Minimum Version to update itself to the Latest Build when it connects and when the Minimum Version is raised, so that raising the Minimum Version updates the fleet without visiting each machine.
30. As a User, I want an Agent that is not updated because the Latest Build is not above its version to stay as it is, so that a missing build never causes a loop or a downgrade.
31. As a User, I want a failed self-update retried every hour while the Agent stays below the Minimum Version, so that a transient failure fixes itself.
32. As a User, I want the reason for the last failed self-update shown under the Device's badge, so that I can fix the cause or delete the Device.
33. As a User, I want the failure reason cleared once the Device reports a version at or above the Minimum Version, or once the Minimum Version is lowered or cleared, so that no stale error lingers.
34. As a person using a Controller (CLI or MCP) below the Minimum Version, I want every command to still run and print one warning line on stderr ("Server recommends rcli ≥ Y, run: rcli update"), so that I know to update without being blocked.
35. As a Device with an Agent below the Minimum Version, I want a self-update not to interrupt an active shell or Live View until the binary is replaced and the Agent restarts, so that update downtime is as short as today's.

### No roles

36. As a User, I want to manage Users, the Server Key, Server settings, Agent Builds, the Minimum Version and Devices without needing an Admin role, so that the whole team can operate the Server.
37. As a User, I want to force-release another User's Control Lock, so that a stuck lock never needs a specific person.
38. As a User, I want to create a User with only a name and password (no role choice), so that adding a teammate is one step.
39. As a User, I want to be unable to delete or disable myself, so that I cannot lock myself out by a misclick.
40. As a User, I want to be unable to delete or disable the last remaining User, so that the Server always has someone who can sign in.
41. As an existing Operator, I want to get every capability after the upgrade with nothing to do, so that the change is invisible except for what I can now do.
42. As a Controller connected by Device Password (no User account), I want to stay limited to that one Device, so that dropping roles never widens Device-Password access.
43. As any User, I want my first `https://` Dashboard visit to be able to record the API Address, so that this no longer waits for an Admin.

### Devices page

44. As a User, I want a `⋯` menu on each Device row with rename, change Device ID, change Device Password, merge, revoke and delete, so that every Device action is where the Device is.
45. As a User, I want delete to ask for confirmation, then remove the Device with its history and disconnect it, so that dead or broken Devices disappear from the list.
46. As a User, I want the separate Admin › Device page gone, so that there is one place for Devices.
47. As a User, I want the "Admin" menu renamed to "Quản lý" with Users, Updates and Settings, so that the navigation matches the absence of roles.

### Removing the old update system

48. As a User, I want `rcli rollout` (keygen, publish, roll out) removed, so that there is one way to publish.
49. As a User, I want the update public key setting and the update key pinned at install removed, so that installs and Server setup need nothing about updates.
50. As a User, I want the per-Device rollout state (pending version, rollout errors) and the "fleet build" concept removed, so that nothing lingers from rollouts.

## Implementation Decisions

### Agent Builds and the Latest Build
- An Agent Build keeps its identity as (version, target). Targets use the existing `<os>-<arch>` form the Agent reports, e.g. `linux-x86_64`.
- The Server validates the version as semver on upload. Latest Build = the highest semver version (a pre-release ranks below its release) that has a file for the target. It is computed on read, never stored.
- Schema:
  - Drop the signature column and the rolled-out timestamp from Agent Builds.
  - Drop the Device columns for the pending update version and the rollout error.
  - Add a nullable "last self-update error" on Devices.
  - Drop the role column from Users.
  - Add an optional Minimum Version to Server settings.
- Upload API: the existing PUT by version and target, without any signature parameter. It refuses a non-semver version and an existing (version, target).
- New delete API: delete one (version, target) and delete a whole version. Each removes the stored object once nothing references it, using the existing unreferenced-object cleanup.
- New list API returning every Agent Build with version, target, size, sha256, uploader and upload time, plus which one is the Latest Build per target.
- The rollout API, the update public key config, the update-key endpoint and its MAC, and Ed25519 sign/verify in the protocol crate are removed. The registration reply no longer carries an update key, and the Controller's machine config drops it.

### Build lookup and integrity
- The build lookup by target (the endpoint `rcli update` and the Agent download from) always serves the Latest Build. The fleet/rolled-out logic and the per-Device variant are removed. The `/download` route for new installs serves the Latest Build too.
- The response keeps the version and sha256 headers and drops the signature header. The client verifies the downloaded file's sha256 against the header, keeps the existing resumable partial-download behaviour, and discards a mismatch.
- **Deliberate change from the grilling:** no MAC is added. The lookup already sends the Server Key's secret in a request header, so a MAC under that same secret would not stop anyone who can read and rewrite the traffic. Integrity against an on-path attacker therefore rests on the API Address being `https://`. ADR 0008 is updated to say so.

### `rcli update`
- Rename the `upgrade` command to `update` and keep `upgrade` as a hidden alias. It compares the running version with the Latest Build's version (semver):
  - Equal: print "already the latest" and exit 0.
  - Running version higher: print "newer than the Server's latest" and exit 0 with no change.
  - Running version lower: download, verify, swap and restart the Agent service, as `upgrade` does today.
- `rcli update` takes no version argument.

### Minimum Version
- Stored in Server settings and editable on the Settings page, alongside the existing settings API. Empty means none.
- Agent self-update reuses the existing `AgentUpdate` message and the Agent's existing download-and-install update task. The Server sends `AgentUpdate` (Latest Build version, target, size, sha256) when all of these hold:
  - a Minimum Version is set;
  - the Agent's hello version is below it;
  - the Latest Build for the Agent's target is above the Agent's version.

  The Server checks this on Agent hello, when the Minimum Version is saved (to every connected Agent), and again every hour per session while the condition holds. The signature field of `AgentUpdate` becomes reserved.
- `UpdateFailed` sets the Device's last self-update error. The error is cleared when the Agent says hello with a version at or above the Minimum Version, or when the Minimum Version is cleared or lowered to at or below the Device's version.
- The Device JSON replaces the old `update` object with `needs_update` (bool: Minimum Version set and Agent version below it) and `update_error` (string or null). The Dashboard Devices page and `rcli devices` render them.
- Controllers: every `/api/v1` response carries a header with the Minimum Version when one is set. The CLI/MCP API client compares it with its own version and prints a single warning line to stderr, at most once per process.

### No roles
- The `Admin` extractor goes away and those routes take the ordinary caller.
- A caller authenticated by Device Password stays restricted to its Device, as today. The restriction that used to come from "admin requires no device scope" must survive: every former admin-only route must refuse a Device-Password caller.
- The API Address is learned from any User's Dashboard request.
- User management refuses deleting or disabling oneself, and deleting or disabling the last enabled User.
- Dashboard:
  - The Devices page gains a per-row `⋯` menu with the actions now on Admin › Device.
  - The Admin › Device page is removed.
  - The "Admin" nav becomes "Quản lý" and is visible to every User.
  - The Users form loses the role field.
  - The Updates page becomes the upload form plus the grouped list with download and delete.
  - Settings gains the Minimum Version field.

### Transition from signed builds
- Agents and Controllers on v0.2.3 and older accept only signed builds. The first release with this spec must reach them through the old flow: a signed `rcli rollout` on the old Server, before the Server itself is upgraded.
- The new Agent must therefore keep working against the old Server. It must accept an `AgentUpdate` that carries a signature, and ignore it.
- Controller-only machines that cannot be reached by that rollout are reinstalled.

## Testing Decisions

- Good tests drive the system from outside: HTTP API calls, CLI commands and Agent sessions against a real Server in the testkit, asserting on what a User sees (API JSON, CLI output, the binary on disk). They do not assert on tables or internal functions.
- **Seam 1, Rust testkit (`TestSystem` / `TestAgent`):** it already boots Server, Agents and CLI together. Rewrite the existing rollout and upgrade test files in place for the new behaviour rather than adding new files.
  - Upload: no signature needed, non-semver refused, duplicate refused.
  - List and delete, including the Latest Build moving back.
  - `rcli update`: updates; already latest; newer than latest; hash mismatch keeps the old binary; partial download resumes. Keep the existing partial-download tests, minus signatures.
  - Minimum Version: an Agent below it receives the Latest Build and reports its new version; an Agent at or above it does not; a failure shows `update_error` and the error clears; `needs_update` appears in the Device JSON.
  - Controller below the Minimum Version: the command succeeds and stderr carries the warning.
  - Roles (extend the existing auth and admin-device tests): a former Operator can do admin actions; a Device-Password caller still cannot; a User cannot delete themselves or the last User.
- **Seam 2, Dashboard Playwright e2e** (existing admin and device-admin specs against the devstack): upload a build and see it listed and marked Latest; delete it; the Devices page `⋯` menu deletes a Device; Settings saves a Minimum Version and the badge appears.
- One test per acceptance criterion. No unit tests for semver ordering or other logic already covered through these seams.

## Out of Scope

- Signing, offline keys or any protection against a compromised Server (ADR 0008).
- Canary rollouts, staged rollouts, auto-update for Devices at or above the Minimum Version, and a Server-side "update this Device" button.
- Installing a specific version, downgrades and rollback by version.
- Blocking Agents or Controllers below the Minimum Version.
- Any role or permission model, including per-User Device scopes for Dashboard Users.
- Building or uploading release binaries from CI.

## Further Notes

- Glossary terms used: Agent Build, Latest Build, Minimum Version, User (all in `GLOSSARY.md`).
- Release order for the first version built from this spec: (1) sign it and `rcli rollout` it with the old Server; (2) upgrade the Server; (3) from then on, upload and use `rcli update` / the Minimum Version.
