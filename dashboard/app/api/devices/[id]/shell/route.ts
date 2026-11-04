import { proxyStream } from "@/lib/api";

export const dynamic = "force-dynamic";

/** Opens an interactive shell on the Device; the reply is the Server's NDJSON stream. Closing it ends the session. */
export async function POST(req: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxyStream(`/api/v1/devices/${encodeURIComponent(id)}/shell`, {
    method: "POST",
    body: await req.text(),
    signal: req.signal,
  });
}
