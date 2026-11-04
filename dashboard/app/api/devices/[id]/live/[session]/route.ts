import { proxy } from "@/lib/api";

export async function DELETE(_: Request, { params }: { params: Promise<{ id: string; session: string }> }) {
  const { id, session } = await params;
  return proxy(`/api/v1/devices/${encodeURIComponent(id)}/live/${encodeURIComponent(session)}`, { method: "DELETE" });
}
