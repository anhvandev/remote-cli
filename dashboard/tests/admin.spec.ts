import { expect, test, type Page } from "@playwright/test";
import { adminApi, devstack, newUser, signIn } from "./devstack";

// Devices `delta`, `echo`, `foxtrot` exist only for these tests (rcli-devstack); alpha/bravo/charlie stay untouched.

/** Signs in an Admin and reaches a sub-page through the Quản trị root. */
async function openAdmin(page: Page, sub: string) {
  await signIn(page, await newUser("admin"));
  await page.getByRole("link", { name: "Quản trị" }).click();
  await expect(page.getByRole("heading", { name: "Quản trị" })).toBeVisible();
  await page.getByRole("main").getByRole("link", { name: sub, exact: true }).click();
}

/** The row whose first cell (the name) matches; other cells may mention other names (e.g. merge choices). */
const rowIn = (page: Page, section: string, text: string | RegExp) =>
  page.getByRole("region", { name: section }).getByRole("row").filter({ has: page.locator("td:first-child", { hasText: text }) });

test("operators don't see the admin screens", async ({ page }) => {
  await signIn(page, await newUser("operator"));
  const nav = page.getByRole("navigation");
  await expect(nav).toContainText("operator");
  await expect(page.getByRole("link", { name: "Quản trị" })).toHaveCount(0);
  for (const name of ["User", "Enrollment Code", "Cập nhật Agent"]) {
    await expect(nav.getByRole("link", { name, exact: true })).toHaveCount(0);
  }
  await expect(nav.getByRole("link", { name: "Device", exact: true })).toHaveCount(1);

  for (const url of ["/admin", "/admin/users", "/admin/devices", "/admin/enrollment", "/admin/updates"]) {
    await page.goto(url);
    await expect(page.getByText("Chỉ admin")).toBeVisible();
    await expect(page.getByRole("region")).toHaveCount(0);
    await expect(page.getByRole("table")).toHaveCount(0);
  }
});

test("the sidebar shows the four Quản trị sub-entries and marks the current one", async ({ page }) => {
  await signIn(page, await newUser("admin"));
  const nav = page.getByRole("navigation");
  const subs: [string, string][] = [
    ["User", "/admin/users"],
    ["Device (Admin)", "/admin/devices"],
    ["Enrollment Code", "/admin/enrollment"],
    ["Cập nhật Agent", "/admin/updates"],
  ];
  for (const [label, path] of subs) {
    await nav.getByRole("link", { name: label, exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`${path}$`));
    await expect(nav.getByRole("link", { name: label, exact: true })).toHaveAttribute("aria-current", "page");
    await expect(nav.getByRole("link", { name: "Quản trị" })).not.toHaveAttribute("aria-current", "page");
  }
  // The top-level Device entry stays unambiguous and inactive on admin pages.
  await expect(nav.getByRole("link", { name: "Device", exact: true })).toHaveCount(1);
});

test("the Quản trị root links to each sub-page", async ({ page }) => {
  await signIn(page, await newUser("admin"));
  await page.getByRole("link", { name: "Quản trị" }).click();
  const main = page.getByRole("main");
  const pages: [string, string][] = [
    ["User", "/admin/users"],
    ["Device", "/admin/devices"],
    ["Enrollment Code", "/admin/enrollment"],
    ["Cập nhật Agent", "/admin/updates"],
  ];
  for (const [name, path] of pages) {
    await main.getByRole("link", { name, exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`${path}$`));
    await page.goBack();
    await expect(page.getByRole("heading", { name: "Quản trị" })).toBeVisible();
  }
});

test("admin creates a user, sets their password, issues and revokes a token, disables them", async ({ page }) => {
  await openAdmin(page, "User");
  const users = page.getByRole("region", { name: "User & API token" });
  const name = `bot-${Date.now()}`;
  await users.getByLabel("Tên user mới").fill(name);
  await users.getByLabel("Vai trò").selectOption("operator");
  await users.getByRole("button", { name: "+ User" }).click();
  const row = rowIn(page, "User & API token", name);
  await expect(row).toContainText("operator");
  await expect(row).toContainText("0 token");

  await row.getByLabel(`Mật khẩu Dashboard cho ${name}`).fill("a long password");
  await row.getByRole("button", { name: "Đặt mật khẩu" }).click();
  await expect(row.getByRole("status").filter({ hasText: "otpauth://" })).toBeVisible();

  await row.getByRole("button", { name: "Tạo token" }).click();
  await expect(row.getByRole("status").filter({ hasText: /rcli_[0-9a-f]{32}/ })).toBeVisible();
  await expect(row).toContainText("1 token");
  page.once("dialog", (d) => d.accept());
  await row.getByRole("button", { name: /Thu hồi token/ }).click();
  await expect(row).toContainText("0 token");

  page.once("dialog", (d) => d.accept());
  await row.getByRole("button", { name: "Vô hiệu" }).click();
  await expect(row).toContainText("đã vô hiệu");
});

test("admin creates a single-use Enrollment Code", async ({ page }) => {
  await openAdmin(page, "Enrollment Code");
  const codes = page.getByRole("region", { name: "Enrollment Code" });
  await codes.getByRole("button", { name: "+ Tạo mã" }).click();
  await expect(codes.getByRole("status")).toContainText("dùng một lần");
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

test("unlock passwords can be set and cleared but are never shown", async ({ page }) => {
  await openAdmin(page, "Device");
  const row = rowIn(page, "Mật khẩu unlock", "bravo");
  await expect(row).toContainText("chưa có");
  const input = row.getByLabel("Mật khẩu unlock cho bravo");
  await expect(input).toHaveAttribute("type", "password");
  await input.fill("s3cret-unlock");
  await row.getByRole("button", { name: "Lưu" }).click();
  await expect(row).toContainText("đã lưu");
  await expect(input).toHaveValue("");
  expect(await page.content()).not.toContain("s3cret-unlock");
  await page.reload();
  expect(await page.content()).not.toContain("s3cret-unlock");

  await row.getByRole("button", { name: "Xoá" }).click();
  await expect(row).toContainText("chưa có");
});

test("the overview shows four cards with counts from the Server", async ({ page }) => {
  await signIn(page, await newUser("admin"));
  await page.getByRole("link", { name: "Quản trị" }).click();
  const main = page.getByRole("main");
  const cards: [string, string][] = [
    ["User", "/admin/users"],
    ["Device", "/admin/devices"],
    ["Cập nhật Agent", "/admin/updates"],
    ["Enrollment Code", "/admin/enrollment"],
  ];
  for (const [name, path] of cards) {
    await expect(page.getByRole("region", { name })).toBeVisible();
    await expect(main.getByRole("link", { name, exact: true })).toHaveAttribute("href", path);
  }

  // Other specs share the devstack, so re-read the Server and the page together until they agree.
  await expect(async () => {
    const users = await adminApi("/api/v1/users");
    const tokens = await adminApi("/api/v1/tokens");
    const devices = await adminApi("/api/v1/devices");
    const unlock = await Promise.all(
      devices.map((d: { id: string }) => adminApi(`/api/v1/devices/${d.id}/unlock-password`)),
    );
    await page.goto("/admin");
    const user = page.getByRole("region", { name: "User" });
    const admins = users.filter((u: { role: string }) => u.role === "admin").length;
    await expect(user).toContainText(`${admins} Admin · ${users.length - admins} Operator`);
    await expect(user).toContainText(`${users.filter((u: { disabled_at: string | null }) => u.disabled_at).length} User đã vô hiệu`);
    await expect(user).toContainText(`${tokens.length} API token`);

    const device = page.getByRole("region", { name: "Device" });
    const count = (s: string) => devices.filter((d: { status: string }) => d.status === s).length;
    await expect(device).toContainText(`${count("Online")} Online · ${count("Offline")} Offline · ${count("Waking")} Waking`);
    await expect(device).toContainText(`${devices.filter((d: { revoked_at: string | null }) => d.revoked_at).length} Device đã thu hồi`);
    await expect(device).toContainText(`${unlock.filter((u: { set: boolean }) => !u.set).length} Device chưa có mật khẩu unlock`);
  }).toPass({ timeout: 20_000 });

  // No API lists Enrollment Codes, so the card offers the action instead of a count.
  const enrollment = page.getByRole("region", { name: "Enrollment Code" });
  await expect(enrollment.getByRole("link", { name: "Tạo Enrollment Code" })).toHaveAttribute("href", "/admin/enrollment");
});

test("the overview lists a failed Agent update with the Device name, error and a link to it", async ({ page }) => {
  const { api, admin_token, update } = await devstack();
  const devices = await adminApi("/api/v1/devices");
  // golf is Online and no other spec asserts on its update state (power-wake only drives its power actions).
  const golf = devices.find((d: { name: string }) => d.name === "golf");

  // Publish the devstack's signed build for a target no Agent reports, then roll it out: the Online
  // Device cannot install it, so it ends up with update.status "failed" and the reason.
  const version = "t07-failed";
  const target = "t07-fake-x86_64";
  const res = await fetch(`${api}/api/v1/agent-builds/${version}/${target}?signature=${update.signature}`, {
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

  await signIn(page, await newUser("admin"));
  await page.getByRole("link", { name: "Quản trị" }).click();
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

test("admin uploads a signed Agent build and rolls it out to selected Devices", async ({ page }) => {
  const { update } = await devstack();
  await openAdmin(page, "Cập nhật Agent");
  const card = page.getByRole("region", { name: "Cập nhật Agent" });
  await card.getByLabel("File build").setInputFiles({ name: "rcli-agent", mimeType: "application/octet-stream", buffer: Buffer.from(update.build) });
  await card.getByLabel("Phiên bản").first().fill(update.version);
  await card.getByLabel("Target").fill(update.target);
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
