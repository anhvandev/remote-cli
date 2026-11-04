"use client";
// Control Lock in the Dashboard: the person acquires, waits for and releases it like any Controller (never pre-empts).
// The lock status comes from the Server's watch stream, which also keeps the lock alive only while this page is
// open: a closed tab frees it 60 s later.
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import type { ControlLock } from "@/lib/api";

const Ctx = createContext<{ device: string; holding: boolean; lock: ControlLock; set: (l: ControlLock) => void }>(null!);

/** True while the signed-in person holds the Device's Control Lock (direct input is allowed). */
export const useHolding = () => useContext(Ctx).holding;
/** The Device's Control Lock as the watch last reported it. */
export const useLock = () => useContext(Ctx).lock;

export function ControlProvider(props: { device: string; me: string; initial: ControlLock; children: ReactNode }) {
  const [lock, set] = useState(props.initial);
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
    <Ctx.Provider value={{ device: props.device, holding: lock.holder === props.me, lock, set }}>{props.children}</Ctx.Provider>
  );
}

export function LockCard() {
  const { device, holding, lock, set } = useContext(Ctx);
  const [waiting, setWaiting] = useState(false);
  const [error, setError] = useState<string>();
  const base = `/api/devices/${encodeURIComponent(device)}/lock`;

  async function post(path: string) {
    const res = await fetch(base + path, { method: "POST" });
    const body = await res.json().catch(() => null);
    if (res.ok) set(body);
    else if (body?.error?.code === "control_lock_held") set({ holder: body.error.holder, expires_at: body.error.expires_at });
    else setError(body?.error?.message ?? res.statusText);
    return res.ok;
  }

  // Waiting: try to acquire each time the watch reports the lock free.
  useEffect(() => {
    if (waiting && !lock.holder) post("").then((ok) => ok && setWaiting(false));
  }, [waiting, lock.holder]); // eslint-disable-line react-hooks/exhaustive-deps

  const time = (t: string) => new Date(t).toLocaleTimeString("vi-VN");
  return (
    <section className="card" aria-label="Control Lock">
      <h3>Control Lock</h3>
      {lock.holder ? (
        <div className="row" style={{ marginBottom: 8 }}>
          <span className="tag lock">🔒 {lock.holder}</span>
          <span className="muted">
            {lock.expires_at ? (
              <>hết hạn lúc <b className="mono">{time(lock.expires_at)}</b></>
            ) : (
              lock.acquired_at && <>giữ từ <b className="mono">{time(lock.acquired_at)}</b></>
            )}
          </span>
        </div>
      ) : (
        <div className="muted" style={{ marginBottom: 8 }}>Không ai giữ</div>
      )}
      {error && <p className="muted">Lỗi: {error}</p>}
      <div className="row">
        {holding ? (
          <button className="btn" onClick={() => post("/release")}>
            Trả Control Lock
          </button>
        ) : waiting ? (
          <>
            <span className="muted">Đang chờ người giữ trả lock…</span>
            <button className="btn" onClick={() => setWaiting(false)}>
              Huỷ chờ
            </button>
          </>
        ) : lock.holder ? (
          <button className="btn primary" onClick={() => (setError(undefined), setWaiting(true))}>
            Chờ Control Lock
          </button>
        ) : (
          <button className="btn primary" onClick={() => (setError(undefined), post(""))}>
            Lấy Control Lock
          </button>
        )}
      </div>
    </section>
  );
}
