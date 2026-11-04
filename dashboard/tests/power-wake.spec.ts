import { expect, test } from "@playwright/test";
import { adminApi, newUser, signIn } from "./devstack";

// Uses delta (Online) and echo/bravo (Offline): other specs don't touch them.
test("power buttons follow the Control Lock and are audited like the CLI's", async ({ page }) => {
  const user = await newUser();
  await adminApi("/api/v1/devices/delta/lock", {});
  let released = false;
  try {
    await signIn(page, user);
    await page.getByRole("link", { name: "delta" }).click();
    const power = page.getByRole("region", { name: "Nguồn" });
    await expect(power.getByRole("button", { name: "Wake" })).toBeDisabled();

    // Someone else holds the Control Lock: only Lock screen is allowed.
    await expect(power).toContainText("Control Lock đang do admin giữ");
    for (const name of ["Restart", "Shut down", "Sleep"]) await expect(power.getByRole("button", { name })).toBeDisabled();
    await power.getByRole("button", { name: "Lock screen" }).click();
    await expect(power).toContainText("Đã gửi Lock screen");

    await adminApi("/api/v1/devices/delta/lock/release", {});
    released = true;
    await expect(power.getByRole("button", { name: "Restart" })).toBeEnabled({ timeout: 15_000 });
    // Destructive actions ask first; dismissing sends nothing.
    page.once("dialog", (d) => d.dismiss());
    await power.getByRole("button", { name: "Shut down" }).click();
    page.once("dialog", (d) => d.accept());
    await power.getByRole("button", { name: "Restart" }).click();
    await expect(power).toContainText("Đã gửi Restart");

    const audit = await adminApi(`/api/v1/audit?device=delta&user=${user.name}`);
    expect(audit.map((e: { action: string; result: object }) => [e.action, e.result])).toEqual([
      ["power.lock", { error: null }],
      ["power.restart", { error: null }],
    ]);
  } finally {
    if (!released) await adminApi("/api/v1/devices/delta/lock/release", {});
  }
});

test("Wake shows why it can't, the last failure, and the Waking state", async ({ page }) => {
  await signIn(page, await newUser());

  // bravo never reported a network interface: no Wake Relay can be chosen.
  await page.getByRole("link", { name: "bravo" }).click();
  const power = page.getByRole("region", { name: "Nguồn" });
  await expect(power.getByRole("button", { name: "Restart" })).toHaveCount(0);
  await power.getByRole("button", { name: "Wake" }).click();
  await expect(power).toContainText("has not reported a network interface with a MAC and an IP address");

  // echo's earlier Wake timed out; alpha is in its LAN and relays the packet.
  await page.goto("/");
  await expect(page.getByRole("row").filter({ hasText: "echo" })).toContainText("Offline (Wake failed)");
  await page.getByRole("link", { name: "echo" }).click();
  await expect(page.getByRole("heading", { name: /echo/ })).toContainText("Offline (Wake failed)");
  await power.getByRole("button", { name: "Wake" }).click();
  await expect(power).toContainText("Đã gửi Wake qua alpha (02:00:00:00:00:05)");
  await expect(page.getByRole("heading", { name: /echo/ })).toContainText("Waking");
  await expect(page.getByRole("heading", { name: /echo/ })).not.toContainText("Wake failed");
});
