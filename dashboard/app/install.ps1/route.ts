const BASE = process.env.RCLI_API_URL ?? "http://localhost:8080";

/** The Windows one-line install script (ticket 17), proxied like `/install.sh`. */
export async function GET(req: Request) {
  const res = await fetch(BASE + "/install.ps1", {
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
}
