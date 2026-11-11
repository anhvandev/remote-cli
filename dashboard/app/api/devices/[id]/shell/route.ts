import { cookies } from "next/headers";
import { SESSION_COOKIE } from "@/lib/api";
import { forward } from "@/lib/forward";

export const dynamic = "force-dynamic";

/** Opens an interactive shell on the Device; the reply is the Server's NDJSON stream. Closing it ends the session. */
export async function POST(req: Request, { params }: { params: Promise<{ id: string }> }) {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) return Response.json({ error: { code: "unauthorized", message: "not signed in" } }, { status: 401 });
  const { id } = await params;
  return forward(req, `/api/v1/devices/${encodeURIComponent(id)}/shell`, {
    "content-type": "application/json",
    authorization: `Bearer ${token}`,
  });
}
