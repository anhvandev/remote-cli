import { expect, test, type Page } from "@playwright/test";
import { adminApi, devstack, newUser, signBuild, signIn } from "./devstack";

// Devices `delta`, `echo`, `foxtrot` exist only for these tests (rcli-devstack); alpha/bravo/charlie stay untouched.

/** Signs in a User and reaches a sub-page through the Quản lý root. */
async function openAdmin(page: Page, sub: string) {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "Quản lý" }).click();
  await expect(page.getByRole("heading", { name: "Quản lý" })).toBeVisible();
  await page.getByRole("main").getByRole("link", { name: sub, exact: true }).click();
}

/** The row whose first cell (the name) matches; other cells may mention other names (e.g. merge choices). */
const rowIn = (page: Page, section: string, text: string | RegExp) =>
  page.getByRole("region", { name: section }).getByRole("row").filter({ has: page.locator("td:first-child", { hasText: text }) });

test("the sidebar shows the Quản lý sub-entries and marks the current one", async ({ page }) => {
  await signIn(page, await newUser());
  const nav = page.getByRole("navigation");
  const subs: [string, string][] = [
    ["User", "/admin/users"],
    ["Cập nhật Agent", "/admin/updates"],
    ["Server Settings", "/admin/settings"],
  ];
  const group = nav.getByRole("group", { name: "Quản lý" });
  for (const [label, path] of subs) {
    await group.getByRole("link", { name: label, exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`${path}$`));
    await expect(group.getByRole("link", { name: label, exact: true })).toHaveAttribute("aria-current", "page");
    await expect(nav.getByRole("link", { name: "Quản lý" })).not.toHaveAttribute("aria-current", "page");
  }
  // The top-level Device entry stays inactive on admin pages.
  await expect(nav.getByRole("link", { name: "Device", exact: true }).first()).not.toHaveAttribute("aria-current", "page");
});

test("the Quản lý root links to each sub-page", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "Quản lý" }).click();
  const main = page.getByRole("main");
  const pages: [string, string][] = [
    ["User", "/admin/users"],
    ["Device", "/admin/devices"],
    ["Cập nhật Agent", "/admin/updates"],
  ];
  for (const [name, path] of pages) {
    await main.getByRole("link", { name, exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`${path}$`));
    await page.goBack();
    await expect(page.getByRole("heading", { name: "Quản lý" })).toBeVisible();
  }
});

test("admin creates a user, sets their password, and disables them", async ({ page }) => {
  await openAdmin(page, "User");
  const users = page.getByRole("region", { name: "User", exact: true });
  const name = `bot-${Date.now()}`;
  await users.getByLabel("Tên user mới").fill(name);
  await expect(users.getByLabel("Vai trò")).toHaveCount(0); // no roles (ADR 0009)
  await users.getByRole("button", { name: "+ User" }).click();
  const row = rowIn(page, "User", name);
  await expect(row).toBeVisible();

  await row.getByLabel(`Mật khẩu Dashboard cho ${name}`).fill("a long password");
  await row.getByRole("button", { name: "Đặt mật khẩu" }).click();
  await expect(row.getByRole("status").filter({ hasText: "otpauth://" })).toBeVisible();

  page.once("dialog", (d) => d.accept());
  await row.getByRole("button", { name: "Vô hiệu" }).click();
  await expect(row).toContainText("đã vô hiệu");
});

test("the fleet shows each Device's 9-digit Device ID", async ({ page }) => {
  const devices = await adminApi("/api/v1/devices");
  const alpha = devices.find((d: { name: string }) => d.name === "alpha");
  await signIn(page, await newUser());
  const row = page.getByRole("row").filter({ hasText: "alpha" });
  await expect(row).toContainText(alpha.device_id);
  expect(alpha.device_id).toMatch(/^\d{9}$/);
});

test("admin renames, revokes, deletes and merges Devices", async ({ page }) => {
  await openAdmin(page, "Device");
  const delta = rowIn(page, "Device", /^delta /);
  await delta.getByLabel("Tên mới cho delta").fill("delta-2");
  await delta.getByRole("button", { name: "Đổi tên" }).click();
  const renamed = rowIn(page, "Device", /^delta-2 /);
  await expect(renamed).toBeVisible();

  page.once("dialog", (d) => d.accept());
  await renamed.getByRole("button", { name: "Thu hồi" }).click();
  await expect(renamed).toContainText("đã thu hồi");

  page.once("dialog", (d) => d.accept());
  await renamed.getByRole("button", { name: "Xoá" }).click();
  await expect(renamed).toHaveCount(0);

  const foxtrot = rowIn(page, "Device", /^foxtrot /);
  await foxtrot.getByLabel("Gộp lịch sử từ Device vào foxtrot").selectOption({ label: "echo" });
  page.once("dialog", (d) => d.accept());
  await foxtrot.getByRole("button", { name: "Gộp" }).click();
  await expect(rowIn(page, "Device", /^echo /)).toHaveCount(0);
  await expect(rowIn(page, "Device", /^foxtrot /)).toBeVisible();
});

test("the overview shows three cards with counts from the Server", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "Quản lý" }).click();
  const main = page.getByRole("main");
  const cards: [string, string][] = [
    ["User", "/admin/users"],
    ["Device", "/admin/devices"],
    ["Cập nhật Agent", "/admin/updates"],
  ];
  for (const [name, path] of cards) {
    await expect(page.getByRole("region", { name })).toBeVisible();
    await expect(main.getByRole("link", { name, exact: true })).toHaveAttribute("href", path);
  }

  // Other specs share the devstack, so re-read the Server and the page together until they agree.
  await expect(async () => {
    const users = await adminApi("/api/v1/users");
    const devices = await adminApi("/api/v1/devices");
    await page.goto("/admin");
    const user = page.getByRole("region", { name: "User" });
    const admins = users.filter((u: { role: string }) => u.role === "admin").length;
    await expect(user).toContainText(`${admins} Admin · ${users.length - admins} Operator`);
    await expect(user).toContainText(`${users.filter((u: { disabled_at: string | null }) => u.disabled_at).length} User đã vô hiệu`);

    const device = page.getByRole("region", { name: "Device" });
    await expect(device).toContainText("6 Online"); // alpha, charlie, golf, india, juliet, kilo: no spec takes them Offline
    const count = (s: string) => devices.filter((d: { status: string }) => d.status === s).length;
    await expect(device).toContainText(`${count("Online")} Online · ${count("Offline")} Offline · ${count("Waking")} Waking`);
    await expect(device).toContainText(`${devices.filter((d: { revoked_at: string | null }) => d.revoked_at).length} Device đã thu hồi`);
  }).toPass({ timeout: 20_000 });
});

test("the overview lists a failed Agent update with the Device name, error and a link to it", async ({ page }) => {
  const { api, admin_token, update } = await devstack();
  const devices = await adminApi("/api/v1/devices");
  // golf is Online and no other spec asserts on its update state (power-wake only drives its power actions).
  const golf = devices.find((d: { name: string }) => d.name === "golf");

  // Publish a signed build for a target no Agent reports, then roll it out: the Online
  // Device cannot install it, so it ends up with update.status "failed" and the reason.
  const version = "t07-failed";
  const target = "t07-fake-x86_64";
  const signature = await signBuild(version, target, update.build);
  const res = await fetch(`${api}/api/v1/agent-builds/${version}/${target}?signature=${signature}`, {
    method: "PUT",
    headers: { authorization: `Bearer ${admin_token}` },
    body: update.build,
  });
  expect(res.status, await res.text()).toBe(201);
  await adminApi(`/api/v1/agent-builds/${version}/rollout`, { devices: [golf.id] });

  await expect
    .poll(async () => {
      const now = await adminApi("/api/v1/devices");
      return now.find((d: { id: string }) => d.id === golf.id)?.update?.status;
    }, { timeout: 15_000 })
    .toBe("failed");
  const error = (await adminApi("/api/v1/devices")).find((d: { id: string }) => d.id === golf.id).update.error;

  await signIn(page, await newUser());
  await page.getByRole("link", { name: "Quản lý" }).click();
  const updates = page.getByRole("region", { name: "Cập nhật Agent" });
  await expect(updates).toContainText(version);
  await expect(updates).toContainText("0/1");
  await expect(updates).toContainText("1 lỗi");
  const link = updates.getByRole("link", { name: `${golf.name}: ${error}` });
  await expect(link).toHaveAttribute("href", `/devices/${golf.id}`);

  // The failed Device's name leads straight to its Tổng quan.
  await link.click();
  await expect(page).toHaveURL(new RegExp(`/devices/${golf.id}$`));
  await expect(page.getByRole("heading", { name: /golf/ })).toBeVisible();
});

test("an upload whose signature does not match is refused with the reason", async ({ page }) => {
  const { update } = await devstack();
  await openAdmin(page, "Cập nhật Agent");
  const card = page.getByRole("region", { name: "Cập nhật Agent" });
  await card.getByLabel("File build").setInputFiles({ name: "rcli", mimeType: "application/octet-stream", buffer: Buffer.from(update.build) });
  // The devstack's signature is for another version, so it does not match this one.
  await card.getByLabel("Phiên bản").first().fill("t02-wrong-signature");
  await card.getByLabel("Hệ điều hành").click();
  await page.getByRole("option", { name: update.target }).click();
  await card.getByLabel("Chữ ký (hex)").fill(update.signature);
  await card.getByRole("button", { name: "Tải lên" }).click();
  await expect(card.getByRole("alert").first()).toContainText("the signature does not match this version, target and file");
});

test("admin uploads a signed Agent build and rolls it out to selected Devices", async ({ page }) => {
  const { update } = await devstack();
  await openAdmin(page, "Cập nhật Agent");
  const card = page.getByRole("region", { name: "Cập nhật Agent" });
  await card.getByLabel("File build").setInputFiles({ name: "rcli", mimeType: "application/octet-stream", buffer: Buffer.from(update.build) });
  await card.getByLabel("Phiên bản").first().fill(update.version);
  await card.getByLabel("Hệ điều hành").click();
  await page.getByRole("option", { name: update.target }).click();
  await card.getByLabel("Chữ ký (hex)").fill(update.signature);
  await card.getByRole("button", { name: "Tải lên" }).click();
  await expect(card.getByRole("status").first()).toContainText(`Đã tải lên ${update.version}`);

  const rollout = page.getByRole("form", { name: "Rollout" });
  await rollout.getByLabel("Phiên bản").fill(update.version);
  await rollout.getByLabel("bravo").check();
  await rollout.getByRole("button", { name: "Cập nhật Device đã chọn" }).click();
  await expect(rowIn(page, "Cập nhật Agent", update.version)).toContainText("0/1");

  // The Device sub-page shows the rollout on the Device itself.
  await page.goto("/admin/devices");
  await expect(rowIn(page, "Device", /^bravo /)).toContainText(`${update.version} · pending`);
});
