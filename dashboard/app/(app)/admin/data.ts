import { api, type Device, type UnlockPassword } from "@/lib/api";

/** Rollout progress per version: Devices it was rolled out to, and how many run it / failed. */
export function groupRollouts(devices: Device[]) {
  const out = new Map<string, { total: number; updated: number; failed: number }>();
  for (const d of devices) {
    if (!d.update) continue;
    const r = out.get(d.update.version) ?? { total: 0, updated: 0, failed: 0 };
    r.total++;
    if (d.update.status === "updated") r.updated++;
    if (d.update.status === "failed") r.failed++;
    out.set(d.update.version, r);
  }
  return out;
}

/** Each Device's unlock password status, in `devices` order. */
export const unlockPasswords = (devices: Device[]) =>
  Promise.all(devices.map((d) => api<UnlockPassword>(`/api/v1/devices/${d.id}/unlock-password`)));
