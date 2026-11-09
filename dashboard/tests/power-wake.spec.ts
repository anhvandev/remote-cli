import { expect, test } from "@playwright/test";
import { adminApi, newUser, signIn } from "./devstack";

// Uses golf (Online) and hotel/bravo (Offline): other specs don't touch them.
test("power buttons follow the Control Lock and are audited like the CLI's", async ({ page }) => {
  const user = await newUser();
  await adminApi("/api/v1/devices/golf/lock", {});
  let released = false;
  try {
    await signIn(page, user);
    await page.getByRole("link", { name: "golf" }).click();
    const power = page.getByRole("region", { name: "Nguồn" });
    await expect(power.getByRole("button", { name: "Wake" })).toBeDisabled();

    // Someone else holds the Control Lock: only Lock screen is allowed.
    await expect(power).toContainText("Control Lock đang do admin giữ");
    for (const name of ["Restart", "Shut down", "Sleep"]) await expect(power.getByRole("button", { name })).toBeDisabled();
    await power.getByRole("button", { name: "Lock screen" }).click();
    await expect(power).toContainText("Đã gửi Lock screen");

    await adminApi("/api/v1/devices/golf/lock/release", {});
    released = true;
    await expect(power.getByRole("button", { name: "Restart" })).toBeEnabled({ timeout: 15_000 });
    // Destructive actions ask first; dismissing sends nothing.
    page.once("dialog", (d) => d.dismiss());
    await power.getByRole("button", { name: "Shut down" }).click();
    page.once("dialog", (d) => d.accept());
    await power.getByRole("button", { name: "Restart" }).click();
    await expect(power).toContainText("Đã gửi Restart");

    const audit = await adminApi(`/api/v1/audit?device=golf&user=${user.name}`);
    expect(audit.map((e: { action: string; result: object }) => [e.action, e.result])).toEqual([
      ["power.lock", { error: null }],
      ["power.restart", { error: null }],
    ]);
  } finally {
    if (!released) await adminApi("/api/v1/devices/golf/lock/release", {});
  }
});

test("Wake shows why it can't", async ({ page }) => {
  await signIn(page, await newUser());

  // bravo never reported a network interface: no Wake Relay can be chosen.
  await page.getByRole("link", { name: "bravo" }).click();
  const power = page.getByRole("region", { name: "Nguồn" });
  await expect(power.getByRole("button", { name: "Restart" })).toHaveCount(0);
  await power.getByRole("button", { name: "Wake" }).click();
  await expect(power).toContainText("has not reported a network interface with a MAC and an IP address");
});

// Wakes hotel: the frozen devstack clock keeps it Waking for the rest of the run, so this stays the last test.
test("an Offline Device shows Wake instead of Live view and Terminal; Waking waits for Online", async ({ page }) => {
  await signIn(page, await newUser());

  // hotel's earlier Wake timed out; alpha is in its LAN and relays the packet.
  await expect(page.getByRole("link", { name: "hotel" })).toBeVisible(); // signed in
  await expect(page.getByRole("row").filter({ hasText: "hotel" })).toContainText("Offline (Wake failed)");
  await page.getByRole("link", { name: "hotel" }).click();
  await expect(page.getByRole("heading", { name: /hotel/ })).toContainText("Offline (Wake failed)");

  const placeholder = page.getByRole("region", { name: "Không xem được" });
  await page.getByRole("link", { name: "Màn hình" }).click();
  await expect(placeholder).toContainText("Device đang Offline");
  await expect(page.getByRole("region", { name: "Live view" })).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Control Lock" })).toHaveCount(0);
  await page.getByRole("link", { name: "Terminal" }).click();
  await expect(placeholder).toContainText("Device đang Offline");
  await expect(page.getByRole("region", { name: "Terminal" })).toHaveCount(0);

  // The placeholder's Wake moves the Device to Waking: both tabs wait for Online, and the header follows.
  await page.getByRole("link", { name: "Màn hình" }).click();
  await placeholder.getByRole("button", { name: "Wake" }).click();
  await expect(page.getByRole("heading", { name: /hotel/ })).toContainText("Waking");
  await expect(page.getByRole("heading", { name: /hotel/ })).not.toContainText("Wake failed");
  await expect(placeholder).toContainText("Đang chờ Device Online…");
  // The header's Wake stays available while Waking and names the relay.
  const power = page.getByRole("region", { name: "Nguồn" });
  await power.getByRole("button", { name: "Wake" }).click();
  await expect(power).toContainText("Đã gửi Wake qua alpha (02:00:00:00:00:05)");
  await page.getByRole("link", { name: "Terminal" }).click();
  await expect(placeholder).toContainText("Đang chờ Device Online…");
  await expect(page.getByRole("region", { name: "Terminal" })).toHaveCount(0);
});
