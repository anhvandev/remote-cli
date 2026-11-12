import { cookies } from "next/headers";
import { SESSION_COOKIE } from "@/lib/api";
import { forward } from "@/lib/forward";

export const dynamic = "force-dynamic";

/** Publishes an Agent build (the Updates page's upload): the body streams to the Server, never held in memory. */
export async function PUT(req: Request, { params }: { params: Promise<{ version: string; target: string }> }) {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) return Response.json({ error: { code: "unauthorized", message: "not signed in" } }, { status: 401 });
  const { version, target } = await params;
  const seg = encodeURIComponent;
  return forward(req, `/api/v1/agent-builds/${seg(version)}/${seg(target)}${new URL(req.url).search}`, {
    "content-type": "application/octet-stream",
    authorization: `Bearer ${token}`,
  });
}
