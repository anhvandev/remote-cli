"use client";
// Control Lock in the Dashboard: the person acquires, waits for and releases it like any Controller (never pre-empts).
// The lock status comes from the Server's watch stream, which also keeps the lock alive only while this page is
// open: a closed tab frees it 60 s later.
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import type { ControlLock } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

/** POSTs to the lock API; `ok` is false when the Server refused, with `error` unless the lock is merely held elsewhere. */
type Post = (path: string) => Promise<{ ok: boolean; error?: string }>;

const Ctx = createContext<{ holding: boolean; lock: ControlLock; post: Post }>(null!);

/** True while the signed-in person holds the Device's Control Lock (direct input is allowed). */
export const useHolding = () => useContext(Ctx).holding;
/** The Device's Control Lock as the watch last reported it. */
export const useLock = () => useContext(Ctx).lock;
/** Releases the Control Lock the signed-in person holds. */
export const useRelease = () => {
  const { post } = useContext(Ctx);
  return () => post("/release");
};

export function ControlProvider(props: { device: string; me: string; initial: ControlLock; children: ReactNode }) {
  const [lock, set] = useState(props.initial);
  const post: Post = async (path) => {
    const res = await fetch(`/api/devices/${encodeURIComponent(props.device)}/lock${path}`, { method: "POST" });
    const body = await res.json().catch(() => null);
    if (res.ok) {
      set(body);
      return { ok: true };
    }
    if (body?.error?.code === "control_lock_held") {
      set({ holder: body.error.holder, expires_at: body.error.expires_at });
      return { ok: false };
    }
    return { ok: false, error: body?.error?.message ?? res.statusText };
  };
  useEffect(() => {
    const abort = new AbortController();
    (async () => {
      const res = await fetch(`/api/devices/${encodeURIComponent(props.device)}/lock/watch`, { signal: abort.signal });
      const reader = res.body!.pipeThrough(new TextDecoderStream()).getReader();
      let buf = "";
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        buf += value;
        const lines = buf.split("\n");
        buf = lines.pop()!;
        if (lines.length) set(JSON.parse(lines[lines.length - 1]));
      }
    })().catch(() => {}); // shortcut: a dropped watch is not reopened; the page's 5 s refresh doesn't restart it either.
    return () => abort.abort();
  }, [props.device]);
  return (
    <Ctx.Provider value={{ holding: lock.holder === props.me, lock, post }}>{props.children}</Ctx.Provider>
  );
}

export function LockCard() {
  const { holding, lock, post } = useContext(Ctx);
  const [waiting, setWaiting] = useState(false);
  const [error, setError] = useState<string>();

  async function send(path: string) {
    const { ok, error: err } = await post(path);
    if (err) setError(err);
    return ok;
  }

  // Waiting: try to acquire each time the watch reports the lock free.
  useEffect(() => {
    if (waiting && !lock.holder) send("").then((ok) => ok && setWaiting(false));
  }, [waiting, lock.holder]); // eslint-disable-line react-hooks/exhaustive-deps

  const time = (t: string) => new Date(t).toLocaleTimeString("vi-VN");
  return (
    <Card aria-label="Control Lock" role="region" className="gap-3">
      <CardHeader>
        <CardTitle className="text-sm font-medium text-muted-foreground">Control Lock</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-3">
        {lock.holder ? (
          <div className="flex flex-wrap items-center gap-2">
            <Badge className="border border-chart-4/25 bg-chart-4/15 text-chart-4">
              🔒 {lock.holder}
            </Badge>
            <span className="text-sm text-muted-foreground">
              {lock.expires_at ? (
                <>hết hạn lúc <b className="font-mono font-medium text-foreground">{time(lock.expires_at)}</b></>
              ) : (
                lock.acquired_at && <>giữ từ <b className="font-mono font-medium text-foreground">{time(lock.acquired_at)}</b></>
              )}
            </span>
          </div>
        ) : (
          <div className="text-sm text-muted-foreground">Không ai giữ</div>
        )}
        {error && <p className="text-sm text-muted-foreground">Lỗi: {error}</p>}
        <div className="flex flex-wrap items-center gap-2">
          {holding ? (
            <Button size="sm" variant="secondary" onClick={() => send("/release")}>
              Trả Control Lock
            </Button>
          ) : waiting ? (
            <>
              <span className="text-sm text-muted-foreground">Đang chờ người giữ trả lock…</span>
              <Button size="sm" variant="ghost" onClick={() => setWaiting(false)}>
                Huỷ chờ
              </Button>
            </>
          ) : lock.holder ? (
            <Button size="sm" onClick={() => (setError(undefined), setWaiting(true))}>
              Chờ Control Lock
            </Button>
          ) : (
            <Button size="sm" onClick={() => (setError(undefined), send(""))}>
              Lấy Control Lock
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
