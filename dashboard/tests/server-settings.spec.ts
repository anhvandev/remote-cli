import { expect, test, type Page } from "@playwright/test";
import { adminApi, newUser, signIn } from "./devstack";

async function openSettings(page: Page) {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "Quản lý" }).click();
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

test("an admin reads the API Address and corrects it", async ({ page }) => {
  await openSettings(page);
  const card = page.getByRole("region", { name: "API Address" });
  const input = card.getByRole("textbox", { name: "API Address" });
  // The devstack Dashboard is plain http on 127.0.0.1, which never teaches the Server its API Address.
  await expect(input).toHaveValue("");

  await input.fill("https://rcli.example.com");
  await card.getByRole("button", { name: "Lưu API Address" }).click();
  await expect(card.getByRole("status")).toContainText("Đã đặt API Address: https://rcli.example.com");

  await page.reload();
  await expect(page.getByRole("region", { name: "API Address" }).getByRole("textbox", { name: "API Address" })).toHaveValue(
    "https://rcli.example.com",
  );
  expect((await adminApi("/api/v1/server-settings")).api_address).toBe("https://rcli.example.com");

  // Only an https:// origin is accepted.
  await page.getByRole("region", { name: "API Address" }).getByRole("textbox", { name: "API Address" }).fill("http://rcli.example.com");
  await page.getByRole("region", { name: "API Address" }).getByRole("button", { name: "Lưu API Address" }).click();
  await expect(page.getByRole("region", { name: "API Address" }).getByRole("alert")).toContainText("https://");
  expect((await adminApi("/api/v1/server-settings")).api_address).toBe("https://rcli.example.com");
});

test("a Minimum Version above a Device's Agent shows the update needed badge on the Devices page", async ({ page }) => {
  await openSettings(page);
  const card = page.getByRole("region", { name: "Minimum Version" });
  await card.getByRole("textbox", { name: "Minimum Version" }).fill("99.0.0");
  await card.getByRole("button", { name: "Lưu Minimum Version" }).click();
  await expect(card.getByRole("status")).toContainText("Đã đặt Minimum Version: 99.0.0");

  await page.goto("/");
  const row = page.getByRole("row").filter({ has: page.getByRole("link", { name: "alpha", exact: true }) });
  await expect(row).toContainText(/update needed · \S+ < 99\.0\.0/);

  // Back to none, so the other specs see no badge.
  await page.goto("/admin/settings");
  await card.getByRole("textbox", { name: "Minimum Version" }).fill("");
  await card.getByRole("button", { name: "Lưu Minimum Version" }).click();
  await expect(card.getByRole("status")).toContainText("Đã bỏ Minimum Version");
});
