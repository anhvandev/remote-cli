import type { Device } from "@/lib/api";

export const day = (t: string) => new Date(t).toLocaleString("vi-VN");

/** Rollout progress per version: Devices it was rolled out to, and how many run it / failed. */
export function rollouts(devices: Device[]) {
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

export const card = "rounded-xl border border-border bg-card";
export const cardHead = "flex flex-wrap items-center justify-between gap-3 border-b border-border px-5 py-3.5";
export const title = "text-sm font-medium text-muted-foreground";
export const nativeSelect =
  "h-8 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50";
