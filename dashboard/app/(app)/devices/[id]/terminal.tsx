"use client";
// Web terminal: an interactive shell on the Device (same run-as rules as `rcli shell`). Output arrives as the Server's
// NDJSON stream through this app's route handler; keystrokes and resizes are separate POSTs. Closing the stream ends it.
import "@xterm/xterm/css/xterm.css";
import type { Terminal as Xterm } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";

type Line = { session?: string; as?: string; output?: string; exit_code?: number; error?: { code: string; message: string } };

export function Terminal({ device }: { device: string }) {
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
    const token = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    const term = new Terminal({ fontSize: 13, theme: { background: token("--color-ink"), foreground: token("--color-paper") } });
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
        return ended(`Không mở được: ${body?.error?.message ?? res.statusText}`);
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
    <Card aria-label="Terminal" role="region" className="gap-3">
      <CardHeader className="flex flex-row items-center justify-between gap-3">
        <CardTitle className="text-base">Terminal</CardTitle>
        <div className="flex items-center gap-3">
          {runAs && (
            <span className="bg-secondary px-2 py-0.5 text-xs font-medium">chạy dưới: {runAs}</span>
          )}
          {state === "open" || state === "connecting" ? (
            <Button size="sm" variant="secondary" onClick={stop}>
              Đóng terminal
            </Button>
          ) : (
            <>
              <div className="flex items-center gap-1.5">
                <Checkbox id="as-root" aria-label="as root" checked={asRoot} onCheckedChange={(v) => setAsRoot(v === true)} />
                <Label htmlFor="as-root" className="text-sm font-medium">
                  as root
                </Label>
              </div>
              <Button size="sm" onClick={open}>
                Mở terminal
              </Button>
            </>
          )}
        </div>
      </CardHeader>
      <CardContent className="grid gap-2">
        {note && <p className="text-sm text-muted-foreground">{note}</p>}
        <div
          ref={box}
          className="overflow-hidden bg-ink"
          style={{ padding: state === "idle" && !note ? 0 : 8, minHeight: state === "idle" && !note ? 0 : 160 }}
        />
      </CardContent>
    </Card>
  );
}
