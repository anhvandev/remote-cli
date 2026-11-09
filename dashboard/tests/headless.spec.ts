import { expect, test } from "@playwright/test";
import { newUser, signIn } from "./devstack";

// juliet (rcli-devstack) is Online with no displays; alpha has a display, so the two tabs differ.
test("a Device with no display hides the Screen tab and says why", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "juliet" }).click();
  await expect(page.getByRole("heading", { name: /juliet/ })).toContainText("Online");

  await expect(page.getByRole("link", { name: "Màn hình" })).toHaveCount(0);
  await expect(page.getByText("Device không có màn hình.")).toBeVisible();
  // Everything else stays.
  const tab = (name: string) => page.getByRole("link", { name });
  for (const name of ["Tổng quan", "Terminal", "Lịch sử"]) await expect(tab(name)).toBeVisible();
  await expect(page.getByRole("region", { name: "Tài nguyên" })).toBeVisible();

  await tab("Terminal").click();
  await expect(tab("Terminal")).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("region", { name: "Terminal" })).toBeVisible();

  // Visiting the Screen URL directly says why instead of showing the Live view.
  await page.goto(page.url().replace(/\/terminal$/, "/screen"));
  // The tab bar says it too, so target the page's own paragraph.
  await expect(page.getByRole("paragraph").filter({ hasText: "Device không có màn hình." })).toBeVisible();
  await expect(page.getByRole("region", { name: "Live view" })).toHaveCount(0);
});

test("a Device with a display keeps the Screen tab", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "alpha" }).click();
  await expect(page.getByRole("link", { name: "Màn hình" })).toBeVisible();
  await expect(page.getByText("Device không có màn hình.")).toHaveCount(0);
});
