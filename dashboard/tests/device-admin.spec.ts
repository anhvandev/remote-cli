import { expect, test } from "@playwright/test";
import { adminApi, devstack, newUser, signIn } from "./devstack";

// Uses kilo: an Online Device no other spec asserts on (renaming it or changing its Device ID would break them).

/** A 9-digit Device ID no current Device holds. */
async function freeId(): Promise<string> {
  const ids = new Set<string>((await adminApi("/api/v1/devices")).map((d: { device_id: string }) => d.device_id));
  for (let n = 100000001; n < 100001000; n++) if (!ids.has(String(n))) return String(n);
  throw new Error("no free Device ID");
}

test("an admin changes a Device's name, password and Device ID", async ({ page }) => {
  await signIn(page, await newUser("admin"));
  await expect(page.getByRole("link", { name: "alpha" })).toBeVisible(); // the session cookie is set
  // Signing in lands on the fleet; the first test renames kilo, so look the Device up by its current name.
  const kilo = (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name.startsWith("kilo"));
  await page.goto(`/devices/${kilo.id}`);
  const card = page.getByRole("region", { name: "Quản trị Device" });

  // Rename: applies at once and the header follows.
  await card.getByLabel("Tên mới").fill("kilo-2");
  await card.getByRole("button", { name: "Đổi tên" }).click();
  await expect(card.getByRole("status")).toContainText("Đã đổi tên");
  await expect(page.getByRole("heading", { name: /kilo-2/ })).toBeVisible();

  // A new Device Password is pushed to the Device and never shown back.
  const password = "kilo-new-password";
  const input = card.getByLabel("Device Password mới");
  await expect(input).toHaveAttribute("type", "password");
  await input.fill(password);
  await card.getByRole("button", { name: "Đặt mật khẩu" }).click();
  await expect(card.getByRole("status")).toContainText("Đã đặt mật khẩu mới");
  await expect(input).toHaveValue("");
  expect(await page.content()).not.toContain(password);
  await page.reload();
  expect(await page.content()).not.toContain(password);

  // A new 9-digit Device ID: the header's ID follows and the Server resolves the Device by it.
  const id = await freeId();
  await card.getByLabel("Device ID mới").fill(id);
  await card.getByRole("button", { name: "Đổi Device ID" }).click();
  await expect(card.getByRole("status")).toContainText("Đã đổi Device ID");
  await expect(card).toContainText(`Device ID ${id}`);
  await expect
    .poll(async () => (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name === "kilo-2")?.device_id)
    .toBe(id);

  // A Device ID another Device holds is refused with a clear error and nothing changes.
  const alpha = (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name === "alpha").device_id;
  await card.getByLabel("Device ID mới").fill(alpha);
  await card.getByRole("button", { name: "Đổi Device ID" }).click();
  await expect(card.getByRole("alert")).toContainText("already used");
  await expect
    .poll(async () => (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name === "kilo-2")?.device_id)
    .toBe(id);
});

test("operators don't see the admin Device actions and the API refuses them", async ({ page, request }) => {
  await signIn(page, await newUser("operator"));
  await expect(page.getByRole("link", { name: "alpha" })).toBeVisible(); // the session cookie is set
  const kilo = (await adminApi("/api/v1/devices")).find((d: { name: string }) => d.name.startsWith("kilo"));
  await page.goto(`/devices/${kilo.id}`);
  await expect(page.getByRole("heading", { name: /kilo/ })).toContainText("Online");
  await expect(page.getByRole("region", { name: "Quản trị Device" })).toHaveCount(0);

  // The Server refuses a direct call holding the operator's session, not just the hidden UI.
  const token = (await page.context().cookies()).find((c) => c.name === "rcli_session")!.value;
  const { api } = await devstack();
  const res = await request.patch(`${api}/api/v1/devices/${kilo.id}`, {
    headers: { authorization: `Bearer ${token}` },
    data: { name: "nope" },
  });
  expect(res.status()).toBe(403);
});
