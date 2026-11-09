import { test, expect } from "@playwright/test";

// The one-line install scripts fetched through the Dashboard origin (ticket 17).
test("install scripts come through the Dashboard origin", async ({ request }) => {
  const sh = await request.get("/install.sh");
  expect(sh.ok()).toBeTruthy();
  const script = await sh.text();
  expect(script.startsWith("#!/bin/sh")).toBeTruthy();
  expect(script).toContain("rcli agent install");
  expect(script).toContain("--key");
  // The script downloads from the origin it was served on (the Dashboard), not the internal API.
  expect(script).toContain(`${new URL(sh.url()).origin}/download/`);

  const ps1 = await request.get("/install.ps1");
  expect(ps1.ok()).toBeTruthy();
  expect(await ps1.text()).toContain("agent install");

  const missing = await request.get("/download/unknown-os/unknown-arch");
  expect(missing.status()).toBe(404);
});
