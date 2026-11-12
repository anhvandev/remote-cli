import { test, expect } from "@playwright/test";
import { devstack } from "./devstack";

// The one-line install scripts fetched through the Dashboard origin (ticket 17).
test("install scripts come through the Dashboard origin", async ({ request }) => {
  const sh = await request.get("/install.sh");
  expect(sh.ok()).toBeTruthy();
  const script = await sh.text();
  expect(script.startsWith("#!/bin/sh")).toBeTruthy();
  expect(script).toContain("rcli install --server");
  expect(script).not.toContain("agent install");
  expect(script).toContain("--key");
  expect(script).not.toContain("rk_"); // the Server Key must never be in a public script
  // The script downloads from the origin it was served on (the Dashboard), not the internal API.
  expect(script).toContain(`${new URL(sh.url()).origin}/download/`);

  const ps1 = await request.get("/install.ps1");
  expect(ps1.ok()).toBeTruthy();
  const powershell = await ps1.text();
  expect(powershell).toContain(" install --server");
  expect(powershell).not.toContain("agent install");
  expect(powershell).toContain("RunAs"); // the one-liner installs the service elevated (UAC)
  expect(powershell).toContain(`${new URL(ps1.url()).origin}/download/windows/`);
  expect(powershell).not.toContain("rk_"); // the Server Key must never be in a public script

  const missing = await request.get("/download/unknown-os/unknown-arch");
  expect(missing.status()).toBe(404);
  expect(await missing.text()).toContain("unknown-os-unknown-arch"); // the error names the target
});

// The download endpoint serves the Latest Build's bytes for an OS/arch, unauthenticated (ticket 17).
test("the download endpoint serves the Latest Build for an OS/arch", async ({ request }) => {
  const { api, admin_token, update } = await devstack();
  const res = await fetch(`${api}/api/v1/agent-builds/1.0.0/linux-x86_64`, {
    method: "PUT",
    headers: { authorization: `Bearer ${admin_token}`, "content-type": "application/octet-stream" },
    body: update.build,
  });
  expect(res.ok).toBeTruthy();
  const dl = await request.get("/download/linux/x86_64");
  expect(dl.ok()).toBeTruthy();
  expect(await dl.body()).toEqual(Buffer.from(update.build));
  expect(dl.headers()["x-rcli-version"]).toBe("1.0.0");
});

// The Connect guide's .dmg/.deb/.rpm links: the package an Admin uploaded, under its own file name.
test("the package download serves the uploaded package with its file name", async ({ request }) => {
  const { api, admin_token } = await devstack();
  const missing = await request.get("/download/package/rpm/aarch64");
  expect(missing.status()).toBe(404);
  const res = await fetch(`${api}/api/v1/packages/deb/x86_64`, {
    method: "PUT",
    headers: { authorization: `Bearer ${admin_token}`, "content-type": "application/octet-stream" },
    body: "a deb",
  });
  expect(res.ok).toBeTruthy();
  const dl = await request.get("/download/package/deb/x86_64");
  expect(dl.ok()).toBeTruthy();
  expect(await dl.text()).toBe("a deb");
  expect(dl.headers()["content-disposition"]).toBe('attachment; filename="rcli-x86_64.deb"');
});
