import { proxy } from "@/lib/api";

/** Streams the Control Lock status (NDJSON, every 5 s). While open, a holder counts as connected. */
export async function GET(req: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxy(`/api/v1/devices/${encodeURIComponent(id)}/lock/watch`, { signal: req.signal });
}
