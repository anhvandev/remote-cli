import { expect, test, type Page } from "@playwright/test";
import { adminApi, newUser, signIn } from "./devstack";

async function openSettings(page: Page) {
  await signIn(page, await newUser("admin"));
  await page.getByRole("link", { name: "Quản trị" }).click();
  await page.getByRole("main").getByRole("link", { name: "Server Settings", exact: true }).click();
  await expect(page).toHaveURL(/\/admin\/settings$/);
}

test("an admin reads the Server Key, replaces it, and registered Devices stay Online", async ({ page }) => {
  await openSettings(page);
  const card = page.getByRole("region", { name: "Server Key" });
  const key = card.locator("code");
  await expect(key).toHaveText(/^rk_[0-9a-f]{64}_[0-9a-f]{32}$/);
  const old = await key.textContent();

  page.once("dialog", (d) => d.accept());
  await card.getByRole("button", { name: "Thay Server Key" }).click();
  await expect(card.getByRole("status")).toContainText("Server Key mới: rk_");
  await expect(key).not.toHaveText(old!);
  // Only the secret changed; the CA fingerprint still pins the same Server.
  expect((await key.textContent())!.split("_")[1]).toBe(old!.split("_")[1]);

  // The replacement does not touch registered Devices.
  const alpha = (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name === "alpha");
  expect(alpha.status).toBe("Online");
  await expect
    .poll(async () => (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name === "alpha").status)
    .toBe("Online");
});

test("an admin changes the Server Address and it persists", async ({ page }) => {
  await openSettings(page);
  const card = page.getByRole("region", { name: "Server Address" });
  const input = card.getByRole("textbox", { name: "Server Address" });
  await expect(input).toHaveValue("localhost");

  // Keep localhost in the list: the devstack Agents reconnect with that name.
  await input.fill("localhost,example.com");
  await card.getByRole("button", { name: "Lưu Server Address" }).click();
  await expect(card.getByRole("status")).toContainText("Đã đặt Server Address: localhost,example.com");

  await page.reload();
  await expect(page.getByRole("region", { name: "Server Address" }).getByRole("textbox", { name: "Server Address" })).toHaveValue(
    "localhost,example.com",
  );

  // Back to what the rest of the suite expects.
  await page.getByRole("region", { name: "Server Address" }).getByRole("textbox", { name: "Server Address" }).fill("localhost");
  await page.getByRole("region", { name: "Server Address" }).getByRole("button", { name: "Lưu Server Address" }).click();
  await expect(page.getByRole("region", { name: "Server Address" }).getByRole("status")).toContainText(
    "Đã đặt Server Address: localhost",
  );
});

test("operators cannot see the Server Settings page", async ({ page }) => {
  await signIn(page, await newUser("operator"));
  const nav = page.getByRole("navigation");
  // Wait for sign-in to finish before navigating; on /signin the nav does not exist yet.
  await expect(nav.getByRole("link", { name: "Device", exact: true }).first()).toBeVisible();
  await expect(nav.getByRole("link", { name: "Server Settings" })).toHaveCount(0);

  await page.goto("/admin/settings");
  await expect(page.getByText("Chỉ Admin mới xem được trang Quản trị.")).toBeVisible();
  await expect(page.getByRole("region")).toHaveCount(0);
});
