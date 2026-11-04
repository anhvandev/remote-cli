"use client";
// Web terminal: an interactive shell on the Device (same run-as rules as `rcli shell`). Output arrives as the Server's
// NDJSON stream through this app's route handler; keystrokes and resizes are separate POSTs. Closing the stream ends it.
import "@xterm/xterm/css/xterm.css";
import type { Terminal as Xterm } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";

type Line = { session?: string; as?: string; output?: string; exit_code?: number; error?: { code: string; message: string } };

export function Terminal({ device, admin }: { device: string; admin: boolean }) {
  const [state, setState] = useState<"idle" | "connecting" | "open" | "ended">("idle");
  const [runAs, setRunAs] = useState<string>();
  const [asRoot, setAsRoot] = useState(false);
  const [note, setNote] = useState<string>();
  const box = useRef<HTMLDivElement>(null);
  const live = useRef<{ term: Xterm; abort: AbortController; cleanup: () => void }>(undefined);

  function close() {
    live.current?.abort.abort();
    live.current?.cleanup();
  }

  function stop() {
    close();
    live.current?.term.dispose();
    live.current = undefined;
    setRunAs(undefined);
    setNote(undefined);
    setState("idle");
  }

  async function open() {
    close();
    live.current?.term.dispose();
    setNote(undefined);
    setRunAs(undefined);
    setState("connecting");
    const [{ Terminal }, { FitAddon }] = await Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit")]);
    const term = new Terminal({ fontSize: 13, theme: { background: "#000", foreground: "#c8f7c5" } });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(box.current!);
    fit.fit();
    const abort = new AbortController();
    const onWindowResize = () => fit.fit();
    window.addEventListener("resize", onWindowResize);
    live.current = { term, abort, cleanup: () => window.removeEventListener("resize", onWindowResize) };
    const ended = (text: string) => {
      if (live.current?.term !== term) return;
      live.current.cleanup();
      setNote(text);
      setState("ended");
    };

    try {
      const res = await fetch(`/api/devices/${encodeURIComponent(device)}/shell`, {
        method: "POST",
        body: JSON.stringify({ cols: term.cols, rows: term.rows, ...(asRoot ? { as: "root" } : {}) }),
        signal: abort.signal,
      });
      if (!res.ok) {
        const body = await res.json().catch(() => null);
        term.dispose();
        return ended(
          body?.error?.code === "no_logged_in_user"
            ? "Không ai đăng nhập trên Device; nhờ admin mở dưới quyền root."
            : `Không mở được: ${body?.error?.message ?? res.statusText}`,
        );
      }
      let session = "";
      let sent = Promise.resolve(); // keystrokes must arrive in order
      const post = (path: string, body: string) => {
        sent = sent.then(() => fetch(`/api/shell-sessions/${session}/${path}`, { method: "POST", body }).then(() => {}, () => {}));
      };
      term.onData((s) => session && post("input", s));
      term.onResize(({ cols, rows }) => session && post("resize", JSON.stringify({ cols, rows })));

      const reader = res.body!.pipeThrough(new TextDecoderStream()).getReader();
      let buf = "";
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return ended("Phiên đã kết thúc.");
        buf += value;
        const lines = buf.split("\n");
        buf = lines.pop()!;
        for (const l of lines.filter(Boolean)) {
          const m: Line = JSON.parse(l);
          if (m.session) {
            session = m.session;
            setRunAs(m.as);
            setState("open");
            term.focus();
          }
          if (m.output) term.write(Uint8Array.from(atob(m.output), (c) => c.charCodeAt(0)));
          if (m.exit_code !== undefined) return ended(`Phiên đã kết thúc (exit ${m.exit_code}).`);
          if (m.error) return ended(`Phiên đã kết thúc: ${m.error.message}`);
        }
      }
    } catch (e) {
      if (abort.signal.aborted) return;
      ended(`Mất kết nối: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  // Leaving the page closes the stream, which ends the session on the Device.
  useEffect(() => () => close(), []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <section className="card" aria-label="Terminal" style={{ gridColumn: "1/-1" }}>
      <div className="row" style={{ justifyContent: "space-between", marginBottom: 10 }}>
        <h3 style={{ margin: 0 }}>Terminal</h3>
        <div className="row">
          {runAs && <span className="tag">chạy dưới: {runAs}</span>}
          {state === "open" || state === "connecting" ? (
            <button className="btn" onClick={stop}>
              Đóng terminal
            </button>
          ) : (
            <>
              {admin && (
                <label className="muted">
                  <input type="checkbox" checked={asRoot} onChange={(e) => setAsRoot(e.target.checked)} /> as root
                </label>
              )}
              <button className="btn primary" onClick={open}>
                Mở terminal
              </button>
            </>
          )}
        </div>
      </div>
      {note && <p className="muted">{note}</p>}
      <div ref={box} style={{ background: "#000", borderRadius: 8, padding: state === "idle" && !note ? 0 : 8 }} />
    </section>
  );
}
