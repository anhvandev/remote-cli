import { expect, test } from "@playwright/test";
import { newUser, signIn } from "./devstack";

test("signed-in pages share a sidebar that marks the current entry", async ({ page }) => {
  await signIn(page, await newUser());
  const nav = page.getByRole("navigation");
  const device = nav.getByRole("link", { name: "Device", exact: true }).first(); // the Quản trị group has its own Device
  await expect(device).toHaveAttribute("aria-current", "page");
  await expect(nav.getByRole("link", { name: "Quản trị" })).toHaveCount(0);
  await expect(nav).toContainText("Đăng xuất");

  // The Device entry stays current on a Device page; the sidebar persists across navigation.
  await page.getByRole("link", { name: "alpha" }).click();
  await expect(page.getByRole("heading", { name: /alpha/ })).toBeVisible();
  await expect(device).toHaveAttribute("aria-current", "page");
});

test("an admin sees Quản trị marked when on it", async ({ page }) => {
  await signIn(page, await newUser("admin"));
  await page.getByRole("link", { name: "Quản trị" }).click();
  await expect(page.getByRole("heading", { name: "Quản trị" })).toBeVisible();
  const nav = page.getByRole("navigation");
  await expect(nav.getByRole("link", { name: "Quản trị" })).toHaveAttribute("aria-current", "page");
  await expect(nav.getByRole("link", { name: "Device", exact: true }).first()).not.toHaveAttribute("aria-current", "page");
});

test("the sign-in page has no sidebar", async ({ page }) => {
  await page.goto("/signin");
  await expect(page.getByRole("button", { name: "Đăng nhập" })).toBeVisible();
  await expect(page.getByRole("navigation")).toHaveCount(0);
});
