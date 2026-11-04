import { expect, test, type Page } from "@playwright/test";
import { devstack, newUser, signIn } from "./devstack";

// Devices `delta`, `echo`, `foxtrot` exist only for these tests (rcli-devstack); alpha/bravo/charlie stay untouched.

async function openAdmin(page: Page) {
  await signIn(page, await newUser("admin"));
  await page.getByRole("link", { name: "Quản trị" }).click();
  await expect(page.getByRole("heading", { name: "Quản trị" })).toBeVisible();
}

/** The row whose first cell (the name) matches; other cells may mention other names (e.g. merge choices). */
const rowIn = (page: Page, section: string, text: string | RegExp) =>
  page.getByRole("region", { name: section }).getByRole("row").filter({ has: page.locator("td:first-child", { hasText: text }) });

test("operators don't see the admin screens", async ({ page }) => {
  await signIn(page, await newUser("operator"));
  await expect(page.getByRole("navigation")).toContainText("operator");
  await expect(page.getByRole("link", { name: "Quản trị" })).toHaveCount(0);
  await page.goto("/admin");
  await expect(page.getByText("Chỉ admin")).toBeVisible();
  await expect(page.getByRole("region", { name: "User & API token" })).toHaveCount(0);
});

test("admin creates a user, sets their password, issues and revokes a token, disables them", async ({ page }) => {
  await openAdmin(page);
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
  await openAdmin(page);
  const codes = page.getByRole("region", { name: "Enrollment Code" });
  await codes.getByRole("button", { name: "+ Tạo mã" }).click();
  await expect(codes.getByRole("status")).toContainText("dùng một lần");
});

test("admin renames, revokes, deletes and merges Devices", async ({ page }) => {
  await openAdmin(page);
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
  await openAdmin(page);
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

test("admin uploads a signed Agent build and rolls it out to selected Devices", async ({ page }) => {
  const { update } = await devstack();
  await openAdmin(page);
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
  await expect(rowIn(page, "Device", /^bravo /)).toContainText(`${update.version} · pending`);
});
