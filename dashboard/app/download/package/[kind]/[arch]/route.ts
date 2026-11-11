import { BASE } from "@/lib/api";

/** An install package (.dmg/.deb/.rpm) an Admin uploaded, proxied unchanged from the Server with its file name. */
export async function GET(_: Request, { params }: { params: Promise<{ kind: string; arch: string }> }) {
  const { kind, arch } = await params;
  const res = await fetch(`${BASE}/download/package/${encodeURIComponent(kind)}/${encodeURIComponent(arch)}`, {
    cache: "no-store",
  });
  const headers = new Headers({ "content-type": res.headers.get("content-type") ?? "application/octet-stream" });
  const disposition = res.headers.get("content-disposition");
  if (disposition) headers.set("content-disposition", disposition);
  return new Response(res.body, { status: res.status, headers });
}
