import { proxy } from "@/lib/api";

/** Keystrokes for a shell session (raw bytes). */
export async function POST(req: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxy(`/api/v1/shell-sessions/${encodeURIComponent(id)}/input`, { method: "POST", body: await req.arrayBuffer() });
}
