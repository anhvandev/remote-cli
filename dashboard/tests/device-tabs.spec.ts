import { expect, test } from "@playwright/test";
import { newUser, signIn } from "./devstack";

const ID = /\/devices\/[0-9a-f-]{36}$/;

test("a Device opens on Tổng quan and each tab has its own URL", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "alpha" }).click();

  await expect(page).toHaveURL(ID);
  const tab = (name: string) => page.getByRole("link", { name });
  await expect(tab("Tổng quan")).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("region", { name: "Tài nguyên" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Phần cứng" })).toBeVisible();
  await expect(page.getByRole("heading", { name: /alpha/ })).toContainText("Online");

  await tab("Màn hình").click();
  await expect(page).toHaveURL(/\/devices\/[0-9a-f-]{36}\/screen$/);
  await expect(tab("Màn hình")).toHaveAttribute("aria-current", "page");
  await expect(tab("Tổng quan")).not.toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("region", { name: "Live view" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Control Lock" })).toBeVisible();
  await expect(page.getByRole("heading", { name: /alpha/ })).toContainText("Online"); // header on every tab

  await tab("Terminal").click();
  await expect(page).toHaveURL(/\/devices\/[0-9a-f-]{36}\/terminal$/);
  await expect(tab("Terminal")).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("region", { name: "Terminal" })).toBeVisible();
  await expect(page.getByRole("heading", { name: /alpha/ })).toContainText("Online");

  await tab("Lịch sử").click();
  await expect(page).toHaveURL(/\/devices\/[0-9a-f-]{36}\/history$/);
  await expect(tab("Lịch sử")).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("region", { name: "Biểu đồ" })).toBeVisible();
  await expect(page.getByRole("heading", { name: /alpha/ })).toContainText("Online");

  await tab("Tổng quan").click();
  await expect(page).toHaveURL(ID);
  await expect(tab("Tổng quan")).toHaveAttribute("aria-current", "page");
});

test("an unknown Device still shows the not-found page", async ({ page }) => {
  await signIn(page, await newUser());
  await expect(page.getByRole("link", { name: "alpha" })).toBeVisible(); // signed in before navigating by URL
  const res = await page.goto("/devices/no-such-device");
  expect(res?.status()).toBe(404);
  await expect(page.getByText(/could not be found/i)).toBeVisible();
});
