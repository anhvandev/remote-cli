import { expect, test } from "@playwright/test";
import { newUser, signIn } from "./devstack";

// alpha is at its lock screen with user "an" logged in: an operator's shell runs as that user. No Control Lock needed.
test("open a web terminal and run a command", async ({ page }) => {
  await signIn(page, await newUser());
  await page.getByRole("link", { name: "alpha" }).click();

  const term = page.getByRole("region", { name: "Terminal" });
  await expect(term.getByLabel("as root")).toHaveCount(0); // operators can't run as root
  await term.getByRole("button", { name: "Mở terminal" }).click();
  await expect(term.getByText("chạy dưới: user")).toBeVisible();
  const screen = term.locator(".xterm-rows");
  await expect(screen).toContainText("$");

  await term.locator(".xterm").click();
  await page.keyboard.type("echo hi-from-web");
  await page.keyboard.press("Enter");
  await expect(screen).toContainText(/hi-from-web\s*\$/);

  await page.keyboard.type("exit 3");
  await page.keyboard.press("Enter");
  await expect(term.getByText("Phiên đã kết thúc (exit 3)")).toBeVisible();
});

test("admins can open the terminal as root", async ({ page }) => {
  await signIn(page, await newUser("admin"));
  await page.getByRole("link", { name: "alpha" }).click();

  const term = page.getByRole("region", { name: "Terminal" });
  await term.getByLabel("as root").check();
  await term.getByRole("button", { name: "Mở terminal" }).click();
  await expect(term.getByText("chạy dưới: root")).toBeVisible();
  await term.getByRole("button", { name: "Đóng terminal" }).click();
  await expect(term.getByRole("button", { name: "Mở terminal" })).toBeVisible();
});
