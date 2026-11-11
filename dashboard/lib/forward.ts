import "server-only";
import { request } from "node:http";
import { Readable } from "node:stream";
import { BASE } from "@/lib/api";

// Hop-by-hop headers, `host` (the upstream's own), and the Dashboard's own mark (lib/api.ts), which no Controller may send.
const HOP = ["host", "connection", "keep-alive", "transfer-encoding", "upgrade", "expect", "x-rcli-dashboard"];

/** Forwards a request to the Controller API at `path` (default: its own path and query) with `extra` headers on top
 * (the Dashboard's routes add the session token). Method, headers and body pass through unchanged, and
 * the reply streams back (shell, `lock/watch`). `X-Forwarded-For` goes on as received (Next fills in the peer when
 * no proxy set it), so the Server, trusting the Dashboard, sees the Controller's IP. When the Controller disconnects
 * the upstream request is closed, so the Server sees the disconnect too. node:http rather than fetch: fetch drops a
 * stream after 300 s without a byte (an idle shell) and a reply after 300 s without headers (a long `exec`). */
export function forward(req: Request, path?: string, extra: Record<string, string> = {}): Promise<Response> {
  const url = new URL(req.url);
  const headers = { ...Object.fromEntries([...req.headers].filter(([k]) => !HOP.includes(k))), ...extra };
  return new Promise((resolve) => {
    // RCLI_API_URL is the internal Controller API, plain http.
    const upstream = request(new URL(path ?? url.pathname + url.search, BASE), { method: req.method, headers }, (res) => {
      const out = new Headers();
      for (const [k, v] of Object.entries(res.headers)) {
        if (v === undefined || HOP.includes(k)) continue;
        for (const value of [v].flat()) out.append(k, value);
      }
      out.set("cache-control", "no-transform"); // keeps Next's gzip from buffering a stream
      const empty = req.method === "HEAD" || res.statusCode === 204 || res.statusCode === 304;
      if (empty) res.resume();
      const body = empty ? null : (Readable.toWeb(res) as ReadableStream);
      resolve(new Response(body, { status: res.statusCode, headers: out }));
    });
    upstream.on("error", () =>
      resolve(Response.json({ error: { code: "bad_gateway", message: "the Controller API is unreachable" } }, { status: 502 })),
    );
    req.signal.addEventListener("abort", () => upstream.destroy());
    if (req.body) Readable.fromWeb(req.body as import("node:stream/web").ReadableStream).pipe(upstream);
    else upstream.end();
  });
}

