import { expect, test, type Locator } from "@playwright/test";
import { adminApi, newUser, signIn } from "./devstack";

/** Frames the video decoded per second over the next 2 s. */
function fps(video: Locator) {
  return video.evaluate(async (v: HTMLVideoElement) => {
    const from = v.getVideoPlaybackQuality().totalVideoFrames;
    await new Promise((r) => setTimeout(r, 2000));
    return (v.getVideoPlaybackQuality().totalVideoFrames - from) / 2;
  });
}

// The viewer is an operator who does not hold the Control Lock: watching does not need it.
// (Not taking the lock here: signin-fleet.spec.ts asserts on alpha's lock in parallel.)
test("watch a Device live over WebRTC and pick a Live View Quality", async ({ page }) => {
  const user = await newUser();
  await signIn(page, user);
  await page.getByRole("link", { name: "alpha" }).click();
  await page.getByRole("link", { name: "Màn hình" }).click();

  const live = page.getByRole("region", { name: "Live view" });
  await live.getByRole("button", { name: "Xem trực tiếp" }).click();
  const video = live.getByLabel("Màn hình alpha");
  // FakePlatform's 1920x1080 display arrives at its native size.
  await expect
    .poll(() => video.evaluate((v: HTMLVideoElement) => `${v.videoWidth}x${v.videoHeight}`), { timeout: 15_000 })
    .toBe("1920x1080");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime > 0 && !v.paused)).toBe(true);

  // Balanced by default. Quality (about 10 fps) and Speed (about 25 fps) take effect on the same stream.
  // Exact rates are the e2e tests' job (crates/testkit/tests/live.rs); here the browser only shows the switch.
  const quality = live.getByLabel("Live View Quality");
  await expect(quality).toHaveValue("balanced");
  await quality.selectOption("quality");
  await expect.poll(() => fps(video), { timeout: 15_000 }).toBeLessThanOrEqual(12);
  await quality.selectOption("speed");
  await expect.poll(() => fps(video), { timeout: 15_000 }).toBeGreaterThanOrEqual(15);
  const starts = (await adminApi(`/api/v1/audit?device=alpha&user=${user.name}`)).filter(
    (e: { action: string }) => e.action === "live.start",
  );
  expect(starts, "no reconnect").toHaveLength(1);

  await live.getByRole("button", { name: "Dừng xem" }).click();
  await expect(live.getByRole("button", { name: "Xem trực tiếp" })).toBeVisible();
  await expect(video).toHaveCount(0);
});
