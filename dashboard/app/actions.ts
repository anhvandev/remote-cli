"use server";
import { cookies, headers } from "next/headers";
import { redirect } from "next/navigation";
import { api, ApiError, json, publicApi, SESSION_COOKIE } from "@/lib/api";

export type SignInState = { error?: string };

export async function signIn(_: SignInState, form: FormData): Promise<SignInState> {
  let session: { token: string };
  try {
    session = await publicApi("/api/v1/sessions", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: form.get("name"), password: form.get("password"), totp: form.get("totp") }),
    });
  } catch (e) {
    if (e instanceof ApiError && e.code === "invalid_credentials") {
      return { error: "Sai tên đăng nhập, mật khẩu hoặc mã TOTP." };
    }
    if (e instanceof ApiError && e.code === "too_many_attempts") {
      return { error: "Đăng nhập sai quá nhiều lần; tài khoản bị khoá tạm 15 phút." };
    }
    throw e;
  }
  const https = (await headers()).get("x-forwarded-proto") === "https";
  (await cookies()).set(SESSION_COOKIE, session.token, {
    httpOnly: true,
    sameSite: "lax",
    secure: https,
    path: "/",
    // No expiry here: the Server expires the session (by its clock) and pages then redirect to /signin.
  });
  redirect("/");
}

export async function signOut() {
  await api("/api/v1/session", { method: "DELETE" }).catch(() => {});
  (await cookies()).delete(SESSION_COOKIE);
  redirect("/signin");
}

/** `error` = the Server's refusal message; `detail` = extra info on success. */
export type Sent = { detail?: string; error?: string };

/** Power Action on a Device; refusals (Control Lock held, not connected...) come back as the Server's message. */
export async function power(device: string, action: "restart" | "shutdown" | "sleep" | "lock"): Promise<Sent> {
  try {
    await api(`/api/v1/devices/${encodeURIComponent(device)}/power`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action }),
    });
    return {};
  } catch (e) {
    if (e instanceof ApiError) return { error: e.message };
    throw e;
  }
}

/** An Admin's change to a Device's identity. The Server pushes a new Device ID or Device Password to the Device
 * (it must be Online) and never returns a password; refusals come back as the Server's message. */
export async function changeDevice(
  device: string,
  change: { name?: string; device_id?: string; device_password?: string },
): Promise<Sent> {
  try {
    await api(`/api/v1/devices/${encodeURIComponent(device)}`, json("PATCH", change));
    return {};
  } catch (e) {
    if (e instanceof ApiError) return { error: e.message };
    throw e;
  }
}

/** Wake via a Wake Relay; `no_wake_relay` etc. come back as the Server's message (it explains why). */
export async function wake(device: string): Promise<Sent> {
  try {
    const r = await api<{ relay: { name: string }; mac: string }>(`/api/v1/devices/${encodeURIComponent(device)}/wake`, {
      method: "POST",
    });
    return { detail: `qua ${r.relay.name} (${r.mac})` };
  } catch (e) {
    if (e instanceof ApiError) return { error: e.message };
    throw e;
  }
}
