import { proxy } from "@/lib/api";

/** Relays the browser's SDP offer to the Device's Agent (WebRTC signaling via the Server). */
export async function POST(req: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxy(`/api/v1/devices/${encodeURIComponent(id)}/live`, { method: "POST", body: await req.text() });
}
