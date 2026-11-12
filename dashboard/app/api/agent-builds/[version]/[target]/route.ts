import { cookies } from "next/headers";
import { SESSION_COOKIE } from "@/lib/api";
import { forward } from "@/lib/forward";

export const dynamic = "force-dynamic";

/** Publishes (PUT) or downloads (GET) an Agent Build from the Updates page: the file streams through, never held in
 * memory. */
async function build(req: Request, { params }: { params: Promise<{ version: string; target: string }> }) {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) return Response.json({ error: { code: "unauthorized", message: "not signed in" } }, { status: 401 });
  const { version, target } = await params;
  const seg = encodeURIComponent;
  return forward(req, `/api/v1/agent-builds/${seg(version)}/${seg(target)}${new URL(req.url).search}`, {
    "content-type": "application/octet-stream",
    authorization: `Bearer ${token}`,
  });
}

export { build as GET, build as PUT };
