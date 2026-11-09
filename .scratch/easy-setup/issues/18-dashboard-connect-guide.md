# 18: Dashboard Connect guide page

**What to build:** A page for every signed-in User with: Server Address and Server Key with copy buttons; install commands per OS (curl, PowerShell) with the key filled in; download links for exe/dmg/deb/rpm; headless install; Controller setup (`rcli setup`, `rcli connect <ID>`); an MCP snippet for Claude Code; how to get past SmartScreen and Gatekeeper for unsigned builds; and a red warning when the Dashboard is served over http. See `.scratch/easy-setup/spec.md` (Dashboard).

**Blocked by:** 17, 10, 05

**Status:** resolved

- [x] Every block is present, with working copy buttons
- [x] The http warning shows only over http
- [x] Playwright test covers the page

## Comments

- Implemented on branch `t/18` (7ebc834 server read rule, 1c1f460 the page). Two vertical slices: the API read
  rule first (testkit red → green), then the page driven by its Playwright spec.
- **Server** (`crates/server/src/api/settings.rs`): `GET /api/v1/server-settings` (show) now takes `Caller`
  instead of `Admin`, so every signed-in User reads the Server Key and Server Address for the guide. A password
  connection may read it too — it already holds the Server Key — and rotate / set-address stay Admin.
  `GET /api/v1/server-key` stays Admin (the `rcli server-key show` surface, pinned by `auth.rs`'s operator
  refusal). `crates/testkit/tests/settings.rs` first test pins the new rule: every signed-in User reads the
  settings; the operator refusals of rotate / set stay covered there.
- **Dashboard** (`dashboard/app/(app)/connect-guide/page.tsx`, `copy-button.tsx`, `http-warning.tsx`, plus a
  "Hướng dẫn kết nối" nav entry for every User): Server Address and Server Key with copy buttons; one-line
  installs with the key filled in (`curl -fsSL <origin>/install.sh | sudo sh -s -- --key …`, PowerShell
  `iwr <origin>/install.ps1 -OutFile install.ps1; .\install.ps1 -Key …`, both shaped like ticket 17's script
  templates); download links for exe/dmg/deb/rpm; headless `sudo rcli agent install --server … --key …`;
  Controller setup (`rcli setup`, `rcli connect <Device ID>`); `claude mcp add rcli -- rcli mcp` (from
  `docs/usage.md`); SmartScreen ("More info → Run anyway") and Gatekeeper (right-click → Open,
  `xattr -d com.apple.quarantine`) notes for the unsigned builds. Copy buttons use
  `navigator.clipboard.writeText()` with an `execCommand` fallback — the Clipboard API is missing on plain http.
- **http warning**: red `role="alert"` banner when `window.location.protocol === "http:"`, unless the request's
  `X-Forwarded-Proto` claims https. Playwright covers both states (the https case sends that header).
  shortcut: a plain-http request that arrives carrying `X-Forwarded-Proto: https` is not warned about — an
  on-path attacker who can set that header can rewrite the page anyway; upgrade if the Dashboard ever serves
  TLS itself.
- **Tests**: `crates/testkit/tests/settings.rs` (3) at the Controller API seam and
  `dashboard/tests/connect-guide.spec.ts` (3 Playwright at 18094/3114): every block present for a signed-in
  Operator with the key filled in the install commands (the page is for every User, so the specs sign in as
  Operator), a copy button landing the Server Key on the clipboard, and the red warning showing over http and
  disappearing behind https. The Playwright spec is also where the copy buttons and the warning are verified —
  no server test is involved in those two.
- Verification on this worktree: `cargo test --workspace` 191 passed / 0 failed; `cargo clippy --workspace
  --all-targets` clean; `cd dashboard && npx tsc --noEmit` clean; Playwright `connect-guide.spec.ts` 3 passed
  (after the spec's strict-mode locator was fixed against Next's route announcer, which is also `role="alert"`).
- Invented where the spec is silent: the download rows point at ticket 17's `/download/<os>/<arch>` endpoint —
  the exe/dmg/deb/rpm packages themselves land with tickets 22/24/16, so today each row serves the raw
  rolled-out build for its OS/arch and deb/rpm share the Linux x86_64 build (a `shortcut:` comment in the page
  says so); the `rcli setup --server` line keeps a `<Controller API URL>` placeholder and fills only the key,
  because the page cannot derive the Controller API's public URL from the Server Address setting (which is
  `host[:port]` for Agents — `rcli setup` wants a URL like `https://server.example.com:8080`); the nav/page
  wording "Hướng dẫn kết nối"; the SmartScreen and Gatekeeper instructions.
- Not verified: the one-line install or any download end to end on a real machine (tickets 16/22/24 territory,
  and the ticket's first checkbox style of verification); the https side of the warning is simulated through
  `X-Forwarded-Proto`, not a real TLS server.

## Comments

- Implemented on `t/18` (3 commits). Every signed-in User can now read `GET /api/v1/server-settings` (show endpoint
  switched from `Admin` to `Caller`; rotate/set-address stay Admin). Dashboard page at
  `dashboard/app/(app)/connect-guide/` with: Server Address + Server Key (copy buttons), install commands
  (curl/PowerShell with key filled in), download links (exe/dmg/deb/rpm), headless install, Controller setup
  (`rcli setup`, `rcli connect <ID>`), MCP snippet (`claude mcp add rcli -- rcli mcp`), SmartScreen/Gatekeeper notes,
  and a red http warning banner. Nav link "Hướng dẫn kết nối" visible to every User.
- Verified: `cargo test --workspace` 191 passed / 0 failed, clippy and `tsc` clean. Playwright
  `connect-guide.spec.ts` 3 passed (ports 18094/3114): all blocks render for Operator, copy button puts the Server
  Key in the real clipboard, http warning appears over http and disappears when `X-Forwarded-Proto: https` is set.
- `shortcut:` (1) `X-Forwarded-Proto: https` suppresses the warning even over plain http — upgrade when Dashboard
  serves its own TLS. (2) Real exe/dmg/deb/rpm packages wait for tickets 16/22/24; download links currently serve
  raw builds from `/download/<os>/<arch>`.
- Not verified: end-to-end install on a real machine; https warning path only simulated via header. `rcli setup
  --server` uses a `<Controller API URL>` placeholder the user must fill in (the page cannot derive it from the
  Server Address alone).
