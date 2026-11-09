import { expect, test, type Page } from "@playwright/test";
import { newUser, signIn } from "./devstack";

test.use({ permissions: ["clipboard-read", "clipboard-write"] });

/** The guide is for every signed-in User (ticket 18), so a plain Operator opens it. */
async function openGuide(page: Page) {
  await signIn(page, await newUser("operator"));
  await page.getByRole("link", { name: "Hướng dẫn kết nối" }).click();
  await expect(page).toHaveURL(/\/connect-guide$/);
  await expect(page.getByRole("heading", { name: "Hướng dẫn kết nối" })).toBeVisible();
}

test("a signed-in User sees every block of the Connect guide", async ({ page }) => {
  await openGuide(page);

  // Server Address and Server Key.
  const key = (await page.getByRole("region", { name: "Server Key" }).locator("code").textContent())!;
  expect(key).toMatch(/^rk_[0-9a-f]{64}_[0-9a-f]{32}$/);
  await expect(page.getByRole("region", { name: "Server Address" }).locator("code")).toHaveText("localhost");

  // Install commands per OS with the key filled in.
  const install = page.getByRole("region", { name: "Cài đặt Agent" });
  await expect(install).toContainText("curl -fsSL");
  await expect(install).toContainText(`/install.sh | sudo sh -s -- --key ${key}`);
  await expect(install).toContainText(`install.ps1 -Key ${key}`);

  // Downloads for exe/dmg/deb/rpm.
  const downloads = page.getByRole("region", { name: "Tải xuống" });
  for (const [name, href] of [
    ["rcli.exe", "/download/windows/x86_64"],
    ["rcli.dmg", "/download/macos/aarch64"],
    ["rcli.deb", "/download/linux/x86_64"],
    ["rcli.rpm", "/download/linux/x86_64"],
  ]) {
    await expect(downloads.getByRole("link", { name })).toHaveAttribute("href", href);
  }

  // Headless install.
  await expect(page.getByRole("region", { name: "Cài đặt không giao diện" })).toContainText(
    `sudo rcli agent install --server localhost --key ${key}`,
  );

  // Controller setup.
  const controller = page.getByRole("region", { name: "Thiết lập Controller" });
  await expect(controller).toContainText("rcli setup");
  await expect(controller).toContainText("rcli connect");

  // MCP snippet for Claude Code.
  await expect(page.getByRole("region", { name: "MCP cho Claude Code" })).toContainText("claude mcp add rcli -- rcli mcp");

  // SmartScreen and Gatekeeper for unsigned builds.
  const unsigned = page.getByRole("region", { name: "SmartScreen và Gatekeeper" });
  await expect(unsigned).toContainText("SmartScreen");
  await expect(unsigned).toContainText("Gatekeeper");
});

test("copy buttons put the value on the clipboard", async ({ page }) => {
  await openGuide(page);
  const keyCard = page.getByRole("region", { name: "Server Key" });
  const key = (await keyCard.locator("code").textContent())!;
  await keyCard.getByRole("button", { name: "Sao chép" }).click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(key);
  await expect(keyCard.getByRole("button", { name: "Đã sao chép" })).toBeVisible();
});

test("the red http warning shows over http and disappears behind https", async ({ page }) => {
  await openGuide(page); // the dev stack serves the Dashboard over http
  await expect(page.getByRole("alert", { name: "Cảnh báo http" })).toContainText("http");
  await page.setExtraHTTPHeaders({ "x-forwarded-proto": "https" });
  await page.reload();
  await expect(page.getByRole("alert", { name: "Cảnh báo http" })).toHaveCount(0);
});
