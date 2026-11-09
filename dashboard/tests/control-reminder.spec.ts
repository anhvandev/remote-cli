import { expect, test } from "@playwright/test";
import { newUser, signIn } from "./devstack";

// Uses india (Online: Màn hình hides the Control Lock while Offline); no other spec takes its Control Lock.
test("the Control Lock reminder shows on every tab and Nhả releases from the header", async ({ page }) => {
  const user = await newUser();
  await signIn(page, user);
  await page.getByRole("link", { name: "india" }).click();
  await page.getByRole("link", { name: "Màn hình" }).click();

  const lock = page.getByRole("region", { name: "Control Lock" });
  await lock.getByRole("button", { name: "Lấy Control Lock" }).click();
  await expect(lock.getByRole("button", { name: "Trả Control Lock" })).toBeVisible();

  const reminder = page.getByText("Bạn đang giữ Control Lock");
  await expect(reminder).toBeVisible();

  // The reminder follows every tab, and switching releases neither it nor the lock.
  for (const tab of ["Tổng quan", "Terminal", "Lịch sử"]) {
    await page.getByRole("link", { name: tab }).click();
    await expect(reminder).toBeVisible();
  }
  await page.getByRole("link", { name: "Màn hình" }).click();
  await expect(lock).toContainText(`🔒 ${user.name}`);
  await expect(lock.getByRole("button", { name: "Trả Control Lock" })).toBeVisible();

  // Nhả in the header releases the lock: the reminder goes and Màn hình shows it free.
  await page.getByRole("link", { name: "Tổng quan" }).click();
  await expect(reminder).toBeVisible();
  await page.getByRole("button", { name: "Nhả" }).click();
  await expect(reminder).toHaveCount(0);
  await page.getByRole("link", { name: "Màn hình" }).click();
  await expect(lock).toContainText("Không ai giữ");
});
