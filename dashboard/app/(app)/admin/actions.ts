"use server";
// Admin server actions. Each returns a Result for <Form/>: the one-time TOTP secret is shown once in
// `message` and never stored by the Dashboard. The Server enforces admin-only on every call.
import { revalidatePath } from "next/cache";
import { api, ApiError } from "@/lib/api";

export type Result = { message?: string; error?: string };

const json = (method: string, body?: unknown): RequestInit => ({
  method,
  headers: { "content-type": "application/json" },
  body: body === undefined ? undefined : JSON.stringify(body),
});
const str = (form: FormData, key: string) => String(form.get(key) ?? "");
const seg = encodeURIComponent;

/** Runs an API call; only a string result is shown (API response bodies are not). */
async function run(fn: () => Promise<unknown>): Promise<Result> {
  try {
    const r = await fn();
    const message = typeof r === "string" ? r : undefined;
    revalidatePath("/admin");
    return { message };
  } catch (e) {
    if (!(e instanceof ApiError)) throw e; // includes Next's redirect to /signin
    return { error: `${e.code}: ${e.message}` };
  }
}

export async function createUser(_: Result, form: FormData) {
  return run(async () => {
    await api("/api/v1/users", json("POST", { name: str(form, "name"), role: str(form, "role") }));
  });
}

export async function setPassword(_: Result, form: FormData) {
  return run(async () => {
    const r = await api<{ totp_secret: string; otpauth_url: string }>(
      `/api/v1/users/${seg(str(form, "user"))}/credentials`,
      json("POST", { password: str(form, "password") }),
    );
    return `TOTP (chỉ hiện một lần): ${r.totp_secret} · ${r.otpauth_url}`;
  });
}

export async function disableUser(_: Result, form: FormData) {
  return run(() => api(`/api/v1/users/${seg(str(form, "user"))}/disable`, json("POST")));
}

const device = (form: FormData) => `/api/v1/devices/${seg(str(form, "device"))}`;

export async function renameDevice(_: Result, form: FormData) {
  return run(() => api(device(form), json("PATCH", { name: str(form, "name") })));
}

export async function revokeDevice(_: Result, form: FormData) {
  return run(() => api(`${device(form)}/revoke`, json("POST")));
}

export async function deleteDevice(_: Result, form: FormData) {
  return run(() => api(device(form), json("DELETE")));
}

export async function mergeDevice(_: Result, form: FormData) {
  return run(() => api(`${device(form)}/merge`, json("POST", { from: str(form, "from") })));
}

export async function uploadBuild(_: Result, form: FormData) {
  return run(async () => {
    const version = str(form, "version");
    const file = form.get("build");
    if (!(file instanceof File) || file.size === 0) throw new ApiError(400, "bad_request", "chọn file build");
    await api(`/api/v1/agent-builds/${seg(version)}/${seg(str(form, "target"))}?signature=${seg(str(form, "signature"))}`, {
      method: "PUT",
      headers: { "content-type": "application/octet-stream" },
      body: await file.arrayBuffer(),
    });
    return `Đã tải lên ${version}`;
  });
}

export async function rollout(_: Result, form: FormData) {
  return run(async () => {
    const devices = form.getAll("devices").map(String);
    const body = form.get("scope") === "all" ? { all: true } : { devices };
    const r = await api<{ devices: number }>(`/api/v1/agent-builds/${seg(str(form, "version"))}/rollout`, json("POST", body));
    return `Đã rollout tới ${r.devices} Device`;
  });
}
