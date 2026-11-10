import { test, expect } from "@playwright/test";
import { devstack } from "./devstack";

// The one-line install scripts fetched through the Dashboard origin (ticket 17).
test("install scripts come through the Dashboard origin", async ({ request }) => {
  const sh = await request.get("/install.sh");
  expect(sh.ok()).toBeTruthy();
  const script = await sh.text();
  expect(script.startsWith("#!/bin/sh")).toBeTruthy();
  expect(script).toContain("rcli agent install");
  expect(script).toContain("--key");
  expect(script).not.toContain("rk_"); // the Server Key must never be in a public script
  // The script downloads from the origin it was served on (the Dashboard), not the internal API.
  expect(script).toContain(`${new URL(sh.url()).origin}/download/`);

  const ps1 = await request.get("/install.ps1");
  expect(ps1.ok()).toBeTruthy();
  const powershell = await ps1.text();
  expect(powershell).toContain("agent install");
  expect(powershell).toContain("RunAs"); // the one-liner installs the service elevated (UAC)
  expect(powershell).toContain(`${new URL(ps1.url()).origin}/download/windows/`);
  expect(powershell).not.toContain("rk_"); // the Server Key must never be in a public script

  const missing = await request.get("/download/unknown-os/unknown-arch");
  expect(missing.status()).toBe(404);
  expect(await missing.text()).toContain("unknown-os-unknown-arch"); // the error names the target
});

// The download endpoint serves the rolled-out build bytes for an OS/arch, unauthenticated (ticket 17).
test("the download endpoint serves the rolled-out build for an OS/arch", async ({ request }) => {
  const { api, admin_token, update } = await devstack();
  // The Server stores, never checks, signatures; any 64-byte hex value will do here.
  const res = await fetch(`${api}/api/v1/agent-builds/1.0.0/linux-x86_64?signature=${"00".repeat(64)}`, {
    method: "PUT",
    headers: { authorization: `Bearer ${admin_token}`, "content-type": "application/octet-stream" },
    body: update.build,
  });
  expect(res.ok).toBeTruthy();
  // Downloads serve the fleet's build: the one rolled out to every Device, not merely published.
  const rollout = await fetch(`${api}/api/v1/agent-builds/1.0.0/rollout`, {
    method: "POST",
    headers: { authorization: `Bearer ${admin_token}`, "content-type": "application/json" },
    body: JSON.stringify({ all: true }),
  });
  expect(rollout.ok).toBeTruthy();
  const dl = await request.get("/download/linux/x86_64");
  expect(dl.ok()).toBeTruthy();
  expect(await dl.body()).toEqual(Buffer.from(update.build));
  expect(dl.headers()["x-rcli-version"]).toBe("1.0.0");
});
