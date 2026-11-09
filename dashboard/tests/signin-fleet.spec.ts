import { expect, test } from "@playwright/test";
import { adminApi, newUser, signIn } from "./devstack";

test("sign in with password + TOTP and open a Device", async ({ page }) => {
  const user = await newUser();
  await signIn(page, user);

  await expect(page.getByRole("heading", { name: /Device/ })).toContainText("6 Online ·"); // admin tests add/remove Offline Devices
  const row = (name: string) => page.getByRole("row").filter({ hasText: name });
  await expect(row("alpha")).toContainText("Online");
  await expect(row("bravo")).toContainText("Offline");
  await expect(page.getByRole("navigation")).toContainText(`${user.name} · Operator`);

  await page.getByRole("link", { name: "alpha" }).click();
  await expect(page.getByRole("heading", { name: /alpha/ })).toContainText("Online");
  const hardware = page.getByRole("region", { name: "Phần cứng" });
  await expect(hardware).toContainText("FakeOS 1.0");
  await expect(hardware).toContainText("Fake CPU 9000 · 8 nhân");
  await expect(hardware).toContainText("eth0 · 02:00:00:00:00:01 · 10.0.0.11");
  await expect(hardware).toContainText("Display 1 · 1920×1080");
  await expect(hardware).toContainText("FAKE-0001");
  const resources = page.getByRole("region", { name: "Tài nguyên" });
  await expect(resources).toContainText("Màn khoá · an");
  await expect(resources).toContainText("CPU13%"); // FakePlatform reports 12.5 %
  await expect(resources).toContainText("4.0 GB / 16.0 GB");

  // The Control Lock card lives on Màn hình.
  await page.getByRole("link", { name: "Màn hình" }).click();
  const lock = page.getByRole("region", { name: "Control Lock" });
  await expect(lock).toContainText("Không ai giữ");

  // Another Controller (the devstack admin) takes the Control Lock; the page shows the holder.
  await adminApi("/api/v1/devices/alpha/lock", {});
  try {
    await page.reload();
    await expect(lock).toContainText("🔒 admin");
  } finally {
    await adminApi("/api/v1/devices/alpha/lock/release", {});
  }
});

test("wrong TOTP code is refused and pages need a session", async ({ page }) => {
  await page.goto("/");
  await expect(page).toHaveURL(/\/signin$/);

  const user = await newUser();
  await signIn(page, { ...user, code: user.code === "000000" ? "111111" : "000000" });
  await expect(page.getByRole("alert").filter({ hasText: "Sai" })).toHaveText("Sai tên đăng nhập, mật khẩu hoặc mã TOTP.");
  await expect(page).toHaveURL(/\/signin$/);
});

test("sign out ends the session", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("button", { name: "Đăng xuất" }).click();
  await expect(page).toHaveURL(/\/signin$/);
  await page.goto("/");
  await expect(page).toHaveURL(/\/signin$/);
});
