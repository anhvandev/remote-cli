import { BASE } from "@/lib/api";

/** Proxies a one-line install script (ticket 17), public: it passes the browser-facing origin along, so the
 * script's download URL points back through the Dashboard instead of the internal API address. */
export function installScript(path: "/install.sh" | "/install.ps1") {
  return async function GET(req: Request) {
    const res = await fetch(BASE + path, {
      headers: {
        "x-forwarded-host": req.headers.get("host") ?? "",
        "x-forwarded-proto": req.headers.get("x-forwarded-proto") ?? "http",
      },
      cache: "no-store",
    });
    return new Response(res.body, {
      status: res.status,
      headers: { "content-type": res.headers.get("content-type") ?? "text/plain" },
    });
  };
}
