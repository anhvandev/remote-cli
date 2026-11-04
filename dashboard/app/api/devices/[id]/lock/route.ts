import { proxy } from "@/lib/api";

/** Acquires the Control Lock (409 `control_lock_held` with holder + expiry if someone else has it). */
export async function POST(_: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxy(`/api/v1/devices/${encodeURIComponent(id)}/lock`, { method: "POST", body: "{}" });
}
