// Fixtures for Dashboard tests: the devstack's admin API access and signing in a fresh user.
import { createHash, createHmac, createPrivateKey, sign } from "node:crypto";
import { readFile } from "node:fs/promises";
import { expect, type Page } from "@playwright/test";

type Info = {
  api: string;
  admin_token: string;
  now: number;
  /** A build signed with the key the devstack's Agents trust. */
  update: { version: string; target: string; build: string; signature: string; signing_key: string };
};

/** Hex update signature of `build` as `version` for `target`, in the format `rcli rollout publish` signs. */
export async function signBuild(version: string, target: string, build: string): Promise<string> {
  const { update } = await devstack();
  const key = createPrivateKey({ key: Buffer.from(update.signing_key, "hex"), format: "der", type: "pkcs8" });
  const sha256 = createHash("sha256").update(build).digest("hex");
  return sign(null, Buffer.from(`rcli-agent-build\n${version}\n${target}\n${sha256}`), key).toString("hex");
}

/** Waits for rcli-devstack to finish seeding the fleet (it writes .devstack.json when ready). */
export async function devstack(): Promise<Info> {
  for (let i = 0; i < 600; i++) {
    try {
      return JSON.parse(await readFile(".devstack.json", "utf8"));
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  throw new Error(".devstack.json not written within 60s");
}

/** Controller API call as the devstack admin. */
export async function adminApi(path: string, body?: unknown) {
  const { api, admin_token } = await devstack();
  const res = await fetch(api + path, {
    method: body === undefined ? "GET" : "POST",
    headers: { authorization: `Bearer ${admin_token}`, "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  expect(res.ok, `${path}: ${res.status}`).toBeTruthy();
  return res.json();
}

/** RFC 6238 code (SHA-1, 30 s, 6 digits) for a base32 secret at `unixSecs` (the devstack's frozen clock). */
export function totp(secretBase32: string, unixSecs: number): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = "";
  for (const c of secretBase32) bits += alphabet.indexOf(c).toString(2).padStart(5, "0");
  const key = Buffer.from(bits.match(/.{8}/g)!.map((b) => parseInt(b, 2)));
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(Math.floor(unixSecs / 30)));
  const mac = createHmac("sha1", key).update(counter).digest();
  const o = mac[19] & 0x0f;
  return String((mac.readUInt32BE(o) & 0x7fffffff) % 1_000_000).padStart(6, "0");
}

let users = 0;

/** Creates a fresh user with a password and TOTP (each code works once), returning what they type to sign in. */
export async function newUser(role: "admin" | "operator" = "operator") {
  const name = `user-${process.pid}-${++users}`;
  const password = "correct horse battery";
  await adminApi("/api/v1/users", { name, role });
  const { totp_secret } = await adminApi(`/api/v1/users/${name}/credentials`, { password });
  return { name, password, code: totp(totp_secret, (await devstack()).now) };
}

export async function signIn(page: Page, user: { name: string; password: string; code: string }) {
  await page.goto("/signin");
  await page.getByLabel("Tên đăng nhập").fill(user.name);
  await page.getByLabel("Mật khẩu").fill(user.password);
  await page.getByLabel("Mã TOTP").fill(user.code);
  await page.getByRole("button", { name: "Đăng nhập" }).click();
}
