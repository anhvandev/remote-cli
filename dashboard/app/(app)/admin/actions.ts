"use server";
// Admin server actions. Each returns a Result for <Form/>: the one-time TOTP secret is shown once in
// `message` and never stored by the Dashboard.
import { api, json } from "@/lib/api";
import { run } from "./run";

export type Result = { message?: string; error?: string };

const str = (form: FormData, key: string) => String(form.get(key) ?? "");
const seg = encodeURIComponent;


export async function createUser(_: Result, form: FormData) {
  return run("/admin", async () => {
    await api("/api/v1/users", json("POST", { name: str(form, "name") }));
  });
}

export async function setPassword(_: Result, form: FormData) {
  return run("/admin", async () => {
    const r = await api<{ totp_secret: string; otpauth_url: string }>(
      `/api/v1/users/${seg(str(form, "user"))}/credentials`,
      json("POST", { password: str(form, "password") }),
    );
    return `TOTP (chỉ hiện một lần): ${r.totp_secret} · ${r.otpauth_url}`;
  });
}

export async function disableUser(_: Result, form: FormData) {
  return run("/admin", () => api(`/api/v1/users/${seg(str(form, "user"))}/disable`, json("POST")));
}

const device = (form: FormData) => `/api/v1/devices/${seg(str(form, "device"))}`;

/** Renames a Device or changes its Device ID or Device Password: whichever of those fields the form has. */
export async function updateDevice(_: Result, form: FormData) {
  const fields = ["name", "device_id", "device_password"].filter((k) => form.has(k));
  const change = Object.fromEntries(fields.map((k) => [k, str(form, k)]));
  return run("/", async () => {
    await api(device(form), json("PATCH", change));
    return "Đã lưu";
  });
}

export async function revokeDevice(_: Result, form: FormData) {
  return run("/", () => api(`${device(form)}/revoke`, json("POST")));
}

export async function deleteDevice(_: Result, form: FormData) {
  return run("/", () => api(device(form), json("DELETE")));
}

export async function mergeDevice(_: Result, form: FormData) {
  return run("/", () => api(`${device(form)}/merge`, json("POST", { from: str(form, "from") })));
}
