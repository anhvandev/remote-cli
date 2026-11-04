import { proxy } from "@/lib/api";

/** The Device's displays (index, size, primary), for the live view's display picker. */
export async function GET(_: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxy(`/api/v1/devices/${encodeURIComponent(id)}/displays`);
}
