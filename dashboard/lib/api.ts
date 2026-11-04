// Controller API access from server components and server actions. The session token lives in an
// httpOnly cookie and never reaches browser JavaScript.
import "server-only";
import { cookies } from "next/headers";
import { redirect } from "next/navigation";

export const SESSION_COOKIE = "rcli_session";
const BASE = process.env.RCLI_API_URL ?? "http://localhost:8080";

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}

/** Calls the Controller API as the signed-in user; sends them to /signin if there is no valid session. */
export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) redirect("/signin");
  const res = await fetch(BASE + path, {
    ...init,
    cache: "no-store",
    headers: { ...init.headers, authorization: `Bearer ${token}` },
  });
  if (res.status === 401) redirect("/signin");
  return parse<T>(res);
}

/** For route handlers that browser code calls: forwards to the Controller API with the session token, and passes
 * the reply (status + JSON) back unchanged. The token stays on the Next server. */
export async function proxy(path: string, init: RequestInit = {}): Promise<Response> {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) return Response.json({ error: { code: "unauthorized", message: "not signed in" } }, { status: 401 });
  const res = await fetch(BASE + path, {
    ...init,
    cache: "no-store",
    headers: { ...init.headers, "content-type": "application/json", authorization: `Bearer ${token}` },
  });
  return new Response(res.status === 204 ? null : await res.text(), {
    status: res.status,
    headers: { "content-type": "application/json" },
  });
}

/** Calls the Controller API without a session (sign-in). */
export async function publicApi<T>(path: string, init: RequestInit): Promise<T> {
  return parse<T>(await fetch(BASE + path, { ...init, cache: "no-store" }));
}

async function parse<T>(res: Response): Promise<T> {
  const body = res.status === 204 ? null : await res.json().catch(() => null);
  if (!res.ok) {
    throw new ApiError(res.status, body?.error?.code ?? "http_error", body?.error?.message ?? res.statusText);
  }
  return body as T;
}

// Shapes of Controller API responses (see crates/server/src/api).
export type User = { id: string; name: string; role: "admin" | "operator" };
export type DeviceStatus = "Online" | "Offline" | "Waking";
export type Device = { id: string; name: string; status: DeviceStatus; last_seen_at: string | null };
export type Inventory = {
  os: string;
  os_version: string;
  hostname: string;
  cpu_model: string;
  cpu_cores: number;
  ram_bytes: number;
  disks: { name: string; mount: string; total_bytes: number }[];
  gpus: string[];
  network_interfaces: { name: string; mac: string; ips: string[] }[];
  serial: string;
  displays: { name: string; width: number; height: number }[];
};
export type Resources = {
  cpu_percent?: number;
  ram_used_bytes?: number;
  disk_used_percent: Record<string, number>;
  disk_read_bytes_per_sec?: number;
  disk_write_bytes_per_sec?: number;
  net_up_bytes_per_sec?: number;
  net_down_bytes_per_sec?: number;
  gpu_percent?: number;
  uptime_secs?: number;
  logged_in_user?: string;
  session?: "Active" | "LockScreen" | "LoginScreen";
  cpu_temp_celsius?: number;
  battery_percent?: number;
  battery_charging?: boolean;
};
/** GET /api/v1/devices/{d}/lock. `expires_at` arrives with the Control Lock rules (ticket 07). */
export type ControlLock = { holder: string | null; acquired_at?: string; expires_at?: string };
export type DeviceDetails = Device & { inventory: Inventory | null; resources: Resources | null };
/** GET /api/v1/devices/{d}/history: raw samples ("5s") or per-minute averages ("1m"), oldest first. */
export type History = { resolution: "5s" | "1m"; samples: { at: string; resources: Resources }[] };
