import { BASE } from "@/lib/api";

/** The Latest Build for one OS/arch (ticket 17), proxied unchanged from the Server. */
export async function GET(_: Request, { params }: { params: Promise<{ os: string; arch: string }> }) {
  const { os, arch } = await params;
  const res = await fetch(`${BASE}/download/${encodeURIComponent(os)}/${encodeURIComponent(arch)}`, { cache: "no-store" });
  const headers = new Headers({ "content-type": res.headers.get("content-type") ?? "application/octet-stream" });
  for (const name of ["x-rcli-version", "x-rcli-sha256"]) {
    const value = res.headers.get(name);
    if (value) headers.set(name, value);
  }
  return new Response(res.body, { status: res.status, headers });
}
