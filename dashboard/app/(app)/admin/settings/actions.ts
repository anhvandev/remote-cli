"use server";
// Server Settings actions. The Server enforces admin-only on every call.
import { revalidatePath } from "next/cache";
import { api, ApiError, type ServerSettings } from "@/lib/api";
import type { Result } from "../actions";

async function run(fn: () => Promise<string>): Promise<Result> {
  try {
    const message = await fn();
    revalidatePath("/admin/settings");
    return { message };
  } catch (e) {
    if (!(e instanceof ApiError)) throw e;
    return { error: `${e.code}: ${e.message}` };
  }
}

export async function rotateServerKey(_: Result, __: FormData) {
  return run(async () => {
    const r = await api<ServerSettings>("/api/v1/server-settings/server-key/rotate", { method: "POST" });
    return `Server Key mới: ${r.server_key}`;
  });
}

export async function setServerAddress(_: Result, form: FormData) {
  return run(async () => {
    const r = await api<{ server_address: string }>("/api/v1/server-settings/server-address", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ address: String(form.get("address") ?? "") }),
    });
    return `Đã đặt Server Address: ${r.server_address}`;
  });
}
