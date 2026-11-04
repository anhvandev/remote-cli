import { expect, test } from "@playwright/test";
import { adminApi, newUser, signIn } from "./devstack";

// Uses charlie: alpha's Control Lock is asserted on by signin-fleet.spec.ts in parallel.
test("wait for the Control Lock, take it, then drive the Device with mouse and keyboard", async ({ page }) => {
  const user = await newUser();
  // Another Controller (the devstack admin) holds the lock.
  await adminApi("/api/v1/devices/charlie/lock", {});
  let released = false;
  try {
    await signIn(page, user);
    await page.getByRole("link", { name: "charlie" }).click();

    const lock = page.getByRole("region", { name: "Control Lock" });
    await expect(lock).toContainText("🔒 admin");
    await expect(lock).toContainText("hết hạn lúc");

    const live = page.getByRole("region", { name: "Live view" });
    await live.getByRole("button", { name: "Xem trực tiếp" }).click();
    const video = live.getByLabel("Màn hình charlie");
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.videoWidth), { timeout: 15_000 }).toBe(1280);
    // Without the lock the view is watch-only.
    await expect(live).toContainText("Chỉ xem: cần giữ Control Lock để điều khiển.");

    // Wait like any Controller: no pre-emption; the lock is taken once the holder lets go.
    await lock.getByRole("button", { name: "Chờ Control Lock" }).click();
    await expect(lock).toContainText("Đang chờ");
    await expect(lock).toContainText("🔒 admin");
    await adminApi("/api/v1/devices/charlie/lock/release", {});
    released = true;
    await expect(lock).toContainText(`🔒 ${user.name}`, { timeout: 15_000 });
    await expect(live).not.toContainText("Chỉ xem");

    // Click the middle of the view, then type: the video is 1280x720 in Observation image space.
    const box = (await video.boundingBox())!;
    await video.click({ position: { x: box.width / 2, y: box.height / 2 } });
    await page.keyboard.press("h");
    await page.keyboard.press("Control+Enter");

    // Each input went through the Action Router and is in the audit log under this user.
    await expect
      .poll(async () => {
        const entries = await adminApi(`/api/v1/audit?device=charlie&user=${user.name}`);
        type Entry = { params: { actions: { x?: number; y?: number }[] }; result: object };
        return entries.map(({ params, result }: Entry) => {
          const { x, y, ...a } = params.actions[0];
          // The view's pixel grid isn't the image's, so a point lands within a pixel of the target.
          return x === undefined ? { ...a, ...result } : { ...a, ...result, near: Math.hypot(x - 640, y! - 360) <= 1.5 };
        });
      })
      .toEqual([
        { type: "click", display: 0, near: true, error: null },
        { type: "type", text: "h", error: null },
        { type: "key", keys: "ctrl+enter", error: null },
      ]);

    await lock.getByRole("button", { name: "Trả Control Lock" }).click();
    await expect(lock).toContainText("Không ai giữ");
    await expect(live).toContainText("Chỉ xem");
  } finally {
    if (!released) await adminApi("/api/v1/devices/charlie/lock/release", {});
  }
});
