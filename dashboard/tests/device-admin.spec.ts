import { expect, test, type Page } from "@playwright/test";
import { adminApi, newUser, signIn } from "./devstack";

// Uses kilo: an Online Device no other spec asserts on (renaming it or changing its Device ID would break them).
// delta, echo, foxtrot exist only for the revoke/delete/merge test (rcli-devstack).

/** A 9-digit Device ID no current Device holds. */
async function freeId(): Promise<string> {
  const ids = new Set<string>((await adminApi("/api/v1/devices")).map((d: { device_id: string }) => d.device_id));
  for (let n = 100000001; n < 100001000; n++) if (!ids.has(String(n))) return String(n);
  throw new Error("no free Device ID");
}

/** The Devices page row of the Device named exactly `name`. */
const row = (page: Page, name: string) =>
  page.getByRole("row").filter({ has: page.getByRole("link", { name, exact: true }) });

/** Opens a row's ⋯ menu and returns it. */
async function menu(page: Page, name: string) {
  await row(page, name).getByRole("button", { name: `Thao tác cho ${name}` }).click();
  return page.getByRole("dialog", { name: `Thao tác cho ${name}` });
}

test("a User changes a Device's name, password and Device ID from its ⋯ menu", async ({ page }) => {
  await signIn(page, await newUser());
  await expect(page.getByRole("link", { name: "alpha" })).toBeVisible(); // the session cookie is set
  const kilo = (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name.startsWith("kilo"));
  const m = await menu(page, kilo.name);

  // Rename: applies at once and the row follows.
  await m.getByLabel("Tên mới").fill("kilo-2");
  await m.getByRole("button", { name: "Đổi tên" }).click();
  await expect(row(page, "kilo-2")).toBeVisible();

  // A new Device Password is pushed to the Device and never shown back.
  const password = "kilo-new-password";
  const input = m.getByLabel("Device Password mới");
  await expect(input).toHaveAttribute("type", "password");
  await input.fill(password);
  await m.getByRole("button", { name: "Đặt mật khẩu" }).click();
  await expect(m.getByRole("status").first()).toContainText("Đã lưu");
  expect(await page.content()).not.toContain(password);

  // A new 9-digit Device ID: the row's ID follows and the Server resolves the Device by it.
  const id = await freeId();
  await m.getByLabel("Device ID mới").fill(id);
  await m.getByRole("button", { name: "Đổi Device ID" }).click();
  await expect(row(page, "kilo-2")).toContainText(id);
  await expect
    .poll(async () => (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name === "kilo-2")?.device_id)
    .toBe(id);

  // A Device ID another Device holds is refused with a clear error and nothing changes.
  const alpha = (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name === "alpha").device_id;
  await m.getByLabel("Device ID mới").fill(alpha);
  await m.getByRole("button", { name: "Đổi Device ID" }).click();
  await expect(m.getByRole("alert")).toContainText("already used");
  expect((await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name === "kilo-2")?.device_id).toBe(id);
});

test("a User revokes, deletes and merges Devices from the ⋯ menu", async ({ page }) => {
  await signIn(page, await newUser());
  await expect(row(page, "delta")).toBeVisible();

  page.once("dialog", (d) => d.accept());
  await (await menu(page, "delta")).getByRole("button", { name: "Thu hồi" }).click();
  await expect(row(page, "delta")).toContainText("đã thu hồi");
  await page.keyboard.press("Escape");

  // Delete asks first, then the Device and its history leave the list.
  let asked = "";
  page.once("dialog", (d) => {
    asked = d.message();
    d.accept();
  });
  await (await menu(page, "delta")).getByRole("button", { name: "Xoá" }).click();
  await expect(row(page, "delta")).toHaveCount(0);
  expect(asked).toContain("Xoá delta");
  await expect.poll(async () => (await adminApi("/api/v1/devices")).some((d: { name: string }) => d.name === "delta")).toBe(false);

  const m = await menu(page, "foxtrot");
  await m.getByLabel("Gộp lịch sử từ Device vào foxtrot").selectOption({ label: "echo" });
  page.once("dialog", (d) => d.accept());
  await m.getByRole("button", { name: "Gộp" }).click();
  await expect(row(page, "echo")).toHaveCount(0);
  await expect(row(page, "foxtrot")).toBeVisible();
});

test("the Admin › Device page is gone", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "Quản lý" }).click();
  await expect(page.getByRole("heading", { name: "Quản lý" })).toBeVisible();
  await expect(page.getByRole("main").getByRole("link", { name: "Device", exact: true })).toHaveCount(0);
  const res = await page.goto("/admin/devices");
  expect(res?.status()).toBe(404);
});
