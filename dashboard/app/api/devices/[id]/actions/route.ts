import { proxy } from "@/lib/api";

/** Direct input from the live view: the Server checks the Control Lock and audits each Action. */
export async function POST(req: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxy(`/api/v1/devices/${encodeURIComponent(id)}/actions`, { method: "POST", body: await req.text() });
}
