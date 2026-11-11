# 17: Install scripts and downloads served by the Server

**What to build:** The Server serves `install.sh`, `install.ps1` and per-OS/arch downloads of the currently rolled-out signed build, through the Dashboard origin. `curl -fsSL https://<dashboard>/install.sh | sudo sh -s -- --key <key>` downloads `rcli` and runs `rcli agent install`. `install.ps1` does the same on Windows and is finished in ticket 22. See `.scratch/easy-setup/spec.md` (Downloads and install scripts).

**Blocked by:** 16

**Status:** resolved

- [ ] The one-line install works end to end on a real Linux machine
- [x] Clear error when no build is rolled out for that OS/arch

## Comments

- Implemented on branch `t/17` (5 commits: `d708c2e`, `73e1c56`, `21f13d1`, `d5e342c`, `test(install): fetch
  scripts and downloads without any credentials`).
- **Server** (`crates/server/src/api/install.rs`, public like the update key — no auth on any of the three):
  `GET /install.sh` and `GET /install.ps1` render a static template with `{{SERVER}}` (the first entry of the
  Server Address setting, what `rcli agent install --server` dials) and `{{ORIGIN}}` (the origin the script was
  fetched from — `X-Forwarded-Host`/`X-Forwarded-Proto`, else the request Host; the Dashboard proxy sets them,
  so the one-liner downloads through the Dashboard origin). `GET /download/<os>/<arch>` serves the signed build
  for the `os-arch` target with `x-rcli-version`/`x-rcli-signature` headers, and 404 `no_build`
  "no build for <os>-<arch>" when there is none. Nothing secret is rendered; the scripts carry
  `cache-control: no-store` (they are per-request, so a shared cache must not serve one origin's rendering to
  the next client).
- **"The build currently rolled out"** is read as `updates::latest` — the newest published signed build for the
  target, the same one `rcli upgrade` fetches. Rollout state is per-Device and a fresh install has no Device
  yet, so the newest build the Admin published is what they made current for that OS/arch.
- **Dashboard** (`app/install.sh`, `app/install.ps1`, `app/download/[os]/[arch]/route.ts`): thin public proxies
  to the Server that pass the browser-facing origin along and forward the version/signature headers.
- **install.sh** takes `--key <key>`, maps `uname` to the build target names (`Darwin` → `macos`), downloads
  `<origin>/download/<os>/<arch>` and runs `rcli agent install --server <Server Address> --key <key>`; a
  missing build prints `install.sh: no rcli build for <os>-<arch> is rolled out`. **install.ps1** does the same
  with `-Key` on Windows (ticket 22 finishes that flow).
- **Tests.** `crates/testkit/tests/install_scripts.rs` (4: scripts carry the key pass-through and the origin's
  download URL and are fetchable with **no credentials**; the rolled-out build bytes come back over
  `/download/<os>/<arch>` and an unknown target 404s with the target named in the message) and
  `dashboard/tests/install-scripts.spec.ts` (2 Playwright specs through the Dashboard origin: script contents
  including no `rk_`/Server-Key leak, and the download endpoint returning the published bytes and
  `x-rcli-version`; run at `RCLI_TEST_API_PORT=18093 RCLI_TEST_WEB_PORT=3113`).
- Verification on the merged tree (this Mac): `cargo test --workspace` 129 passed / 0 failed (including
  `install_scripts` 4); `cargo clippy --workspace --all-targets` clean; `cd dashboard && npx tsc --noEmit`
  clean; Playwright 2 passed.
- shortcut: `rcli agent install` (which installs the service) lands with tickets 16/22/24 — until then the
  script's last step fails with "no such command" on a real machine. `install.sh` passes only `--key` through
  (`--allow-root` / `--name` passthrough can be added when ticket 16 settles the install flags).
- Not verified: the one-line install end to end on a real Linux machine (needs a real machine and ticket 16's
  `rcli agent install`) — the first checkbox stays open. What is verified here is everything down to and
  including the served script contents and the download endpoint returning the build.
- Invented: the `{{ORIGIN}}`/`{{SERVER}}` placeholders; the "newest published build" reading of "currently
  rolled out" (above); the script's missing-build message wording.

## Comments

- Implemented across two overlapping agent runs on `t/17` (6 commits). Server routes `/install.sh`, `/install.ps1`,
  and `/download/<os>/<arch>` (public, no credentials — the Agent's signature check is the real gate), proxied through
  the Dashboard origin. `install.sh` maps `uname` to build targets (fixing `Darwin`→`macos`), prints a clear error when
  no build is rolled out, and sets `cache-control: no-store`. `install.ps1` for Windows.
- Semantics of "currently rolled out": the newest published build for the target (`updates::latest`), matching
  `rcli upgrade` — rollout is per-Device and a new install has no Device row.
- Verified: `cargo test --workspace` 191 passed / 0 failed (incl. `install_scripts` 4), clippy and `tsc` clean,
  Playwright 2/2 passed through the Dashboard origin (ports 18087/3107).
- **Not verified:** "The one-line install works end to end on a real Linux machine" — needs a real Linux machine and
  ticket 16's `rcli agent install`. The script passes `--key` but not `--allow-root`/`--name`.

- code-review fix (round 4): install packages. `rcli package publish <file> --arch <arch>` (Admin) stores a .dmg,
  .deb or .rpm, one per kind and arch; `GET /download/package/<kind>/<arch>` serves it to anyone as
  `rcli-<arch>.<kind>`, also through the Dashboard origin.
