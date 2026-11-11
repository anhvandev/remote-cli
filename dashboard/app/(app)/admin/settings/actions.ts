"use server";
// Server Settings actions. The Server enforces admin-only on every call.
import { api, json, type ServerSettings } from "@/lib/api";
import type { Result } from "../actions";
import { run } from "../run";

export async function rotateServerKey(_: Result, __: FormData) {
  return run("/admin/settings", async () => {
    const r = await api<ServerSettings>("/api/v1/server-settings/server-key/rotate", { method: "POST" });
    return `Server Key mới: ${r.server_key}`;
  });
}

export async function setServerAddress(_: Result, form: FormData) {
  return run("/admin/settings", async () => {
    const r = await api<{ server_address: string }>(
      "/api/v1/server-settings/server-address",
      json("PUT", { address: String(form.get("address") ?? "") }),
    );
    return `Đã đặt Server Address: ${r.server_address}`;
  });
}

export async function setApiAddress(_: Result, form: FormData) {
  return run("/admin/settings", async () => {
    const r = await api<{ api_address: string }>(
      "/api/v1/server-settings/api-address",
      json("PUT", { address: String(form.get("address") ?? "") }),
    );
    return `Đã đặt API Address: ${r.api_address}`;
  });
}
