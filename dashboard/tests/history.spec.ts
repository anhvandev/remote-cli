import { expect, test } from "@playwright/test";
import { newUser, signIn } from "./devstack";

test("Device detail charts resource history; long ranges read 1-minute rollups", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "alpha" }).click();
  await page.getByRole("link", { name: "Lịch sử" }).click();

  const history = page.getByRole("region", { name: "Biểu đồ" });
  // Default range: the last hour from raw 5 s samples.
  await expect(history.getByRole("link", { name: "1h" })).toHaveAttribute("aria-current", "page");
  await expect(history).toContainText("mẫu 5 giây");
  for (const chart of ["CPU", "RAM", "Disk", "Mạng", "GPU"]) {
    await expect(history.getByRole("img", { name: chart, exact: true })).toBeVisible();
  }
  await expect(history.getByRole("figure", { name: "CPU" })).toContainText("13%"); // FakePlatform reports 12.5 %
  await expect(history.getByRole("figure", { name: "RAM" })).toContainText("4.0 GB");

  // 24h switches to the per-minute rollups (devstack has a completed minute of samples).
  await history.getByRole("link", { name: "24h" }).click();
  await expect(page).toHaveURL(/\/devices\/[0-9a-f-]{36}\/history\?range=24h$/); // the range links stay on this tab
  await expect(history.getByRole("link", { name: "24h" })).toHaveAttribute("aria-current", "page");
  await expect(history).toContainText("trung bình mỗi phút");
  await expect(history.getByRole("figure", { name: "CPU" })).toContainText("13%");
});
