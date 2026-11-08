import { expect, test } from "@playwright/test";
import { newUser, signIn } from "./devstack";

// The viewer is an operator who does not hold the Control Lock: watching does not need it.
// (Not taking the lock here: signin-fleet.spec.ts asserts on alpha's lock in parallel.)
test("watch a Device live over WebRTC", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "alpha" }).click();
  await page.getByRole("link", { name: "Màn hình" }).click();

  const live = page.getByRole("region", { name: "Live view" });
  await live.getByRole("button", { name: "Xem trực tiếp" }).click();
  const video = live.getByLabel("Màn hình alpha");
  // FakePlatform's 1920x1080 display arrives downscaled like Observations (long edge 1280).
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => `${v.videoWidth}x${v.videoHeight}`), { timeout: 15_000 })
    .toBe("1280x720");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime > 0 && !v.paused)).toBe(true);

  await live.getByRole("button", { name: "Dừng xem" }).click();
  await expect(live.getByRole("button", { name: "Xem trực tiếp" })).toBeVisible();
  await expect(video).toHaveCount(0);
});
