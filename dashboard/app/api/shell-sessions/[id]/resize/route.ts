import { proxy } from "@/lib/api";

/** New terminal size `{cols, rows}` for a shell session. */
export async function POST(req: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxy(`/api/v1/shell-sessions/${encodeURIComponent(id)}/resize`, { method: "POST", body: await req.text() });
}
