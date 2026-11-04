"use server";
import { cookies, headers } from "next/headers";
import { redirect } from "next/navigation";
import { api, ApiError, publicApi, SESSION_COOKIE } from "@/lib/api";

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
