"use server";
// Server Settings actions.
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

export async function setMinimumVersion(_: Result, form: FormData) {
  return run("/admin/settings", async () => {
    const r = await api<{ minimum_version: string }>(
      "/api/v1/server-settings/minimum-version",
      json("PUT", { version: String(form.get("version") ?? "") }),
    );
    return r.minimum_version ? `Đã đặt Minimum Version: ${r.minimum_version}` : "Đã bỏ Minimum Version";
  });
}
