import { expect, test, type Page } from "@playwright/test";
import { newUser, signIn } from "./devstack";

// alpha/charlie/golf/india stay Online, bravo/hotel stay Offline; other specs mutate delta/echo/foxtrot, so filter by names.
const row = (page: Page, name: string) => page.getByRole("row").filter({ hasText: name });

test("search narrows the fleet by name, case-insensitively, and lands in the URL", async ({ page }) => {
  await signIn(page, await newUser());
  await expect(row(page, "alpha")).toBeVisible();

  await page.getByRole("searchbox", { name: "Tìm Device" }).fill("ALP");

  await expect(page).toHaveURL(/[?&]q=ALP(?:&|$)/);
  await expect(row(page, "alpha")).toBeVisible();
  await expect(row(page, "bravo")).toHaveCount(0);
  // The counts line still describes the whole fleet, not the filtered list.
  await expect(page.getByRole("heading", { name: /Device/ })).toContainText("5 Online");
});

test("status filter narrows the fleet and lands in the URL", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("combobox", { name: "Trạng thái" }).click();
  await page.getByRole("option", { name: "Offline" }).click();

  await expect(page).toHaveURL(/[?&]status=Offline(?:&|$)/);
  await expect(row(page, "bravo")).toBeVisible();
  await expect(row(page, "alpha")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: /Device/ })).toContainText("5 Online");
});

test("a filtered link opens filtered, and Back from a Device restores it", async ({ page }) => {
  await signIn(page, await newUser());
  await expect(row(page, "alpha")).toBeVisible();
  await page.goto("/?q=alp&status=Online");
  await expect(row(page, "alpha")).toBeVisible();
  await expect(row(page, "bravo")).toHaveCount(0);

  await page.getByRole("link", { name: "alpha" }).click();
  await expect(page.getByRole("heading", { name: /alpha/ })).toContainText("Online");

  await page.goBack();
  await expect(page).toHaveURL(/[?&]q=alp/);
  await expect(page).toHaveURL(/[?&]status=Online/);
  await expect(row(page, "alpha")).toBeVisible();
  await expect(row(page, "bravo")).toHaveCount(0);
});

test("no match says so, distinctly from an empty fleet", async ({ page }) => {
  await signIn(page, await newUser());
  await expect(row(page, "alpha")).toBeVisible();

  await page.goto("/?q=zzz-no-such-device");
  await expect(page.getByText("Không có Device nào khớp.")).toBeVisible();
  await expect(page.getByText("Chưa có Device nào.")).toHaveCount(0);
  await expect(page.getByRole("row").filter({ hasText: "alpha" })).toHaveCount(0);
});
