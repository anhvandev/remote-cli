"use client";
// Live view: the browser is a WebRTC peer of the Device's Agent. Signaling goes through this app's route handlers
// (app/api/...) to the Server; media flows peer-to-peer, or through TURN when NAT blocks a direct path.
// While the person holds the Control Lock, their mouse and keyboard on the view become Actions (POST .../actions),
// which the Server checks against the lock and audits like any Controller's.
// The video comes at the display's native size; each viewer picks a Live View Quality for their own stream.
import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent, type WheelEvent } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { nativeSelect } from "../../styles";
import { useHolding } from "./control";

/** Waits until all ICE candidates are in the offer (the Server relays one offer, no trickle); gives up after 5 s. */
function gathered(pc: RTCPeerConnection) {
  return new Promise<void>((done) => {
    if (pc.iceGatheringState === "complete") return done();
    pc.addEventListener("icegatheringstatechange", () => pc.iceGatheringState === "complete" && done());
    setTimeout(done, 5000);
  });
}

async function json(res: Response) {
  const body = await res.json().catch(() => null);
  if (!res.ok) throw new Error(body?.error?.message ?? res.statusText);
  return body;
}

/** Browser key -> Action key combo ("ctrl+enter"); plain characters are typed as text instead. */
function keys(e: KeyboardEvent) {
  const mods = [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "meta"].filter(Boolean);
  const key = e.key === " " ? "space" : e.key.replace(/^Arrow/, "").toLowerCase();
  return [...mods, key].join("+");
}

export function LiveView({ device, name }: { device: string; name: string }) {
  const holding = useHolding();
  const [state, setState] = useState<"idle" | "connecting" | "live">("idle");
  const [error, setError] = useState<string>();
  const [inputError, setInputError] = useState<string>();
  const queue = useRef(Promise.resolve());
  const click = useRef<{ timer?: ReturnType<typeof setTimeout>; send?: () => void; down?: { x: number; y: number }; dragged?: boolean }>({});
  const [stream, setStream] = useState<MediaStream>();
  const [displays, setDisplays] = useState<{ index: number; primary: boolean; width: number; height: number }[]>([]);
  // The streamed display; input Actions target it.
  const [display, setDisplay] = useState(0);
  const live = useRef<{ pc: RTCPeerConnection; picker: RTCDataChannel; session?: string }>(undefined);
  const video = useRef<HTMLVideoElement>(null);
  const quality = useRef<HTMLSelectElement>(null);
  const base = `/api/devices/${encodeURIComponent(device)}/live`;

  function stop() {
    const l = live.current;
    live.current = undefined;
    if (l) {
      l.pc.close();
      if (l.session) fetch(`${base}/${l.session}`, { method: "DELETE", keepalive: true }).catch(() => {});
    }
    setStream(undefined);
    setState("idle");
  }

  /** Starts streaming `pick`, or the primary display. */
  async function start(pick?: number) {
    setError(undefined);
    setState("connecting");
    try {
      const list = await json(await fetch(`/api/devices/${encodeURIComponent(device)}/displays`));
      setDisplays(list);
      const shown: number = pick ?? list.find((d: { primary: boolean }) => d.primary)?.index ?? 0;
      setDisplay(shown);
      const pc = new RTCPeerConnection({ iceServers: await json(await fetch("/api/ice-servers")) });
      // The Agent opens the same negotiated channel; it hears this viewer's Live View Quality on it.
      const picker = pc.createDataChannel("quality", { negotiated: true, id: 0 });
      picker.onopen = () => picker.send(quality.current?.value ?? "balanced");
      live.current = { pc, picker };
      pc.addTransceiver("video", { direction: "recvonly" });
      pc.ontrack = (e) => setStream(e.streams[0] ?? new MediaStream([e.track]));
      pc.onconnectionstatechange = () => {
        if (pc.connectionState === "failed" && live.current?.pc === pc) {
          stop();
          setError("Mất kết nối tới Device.");
        }
      };
      await pc.setLocalDescription(await pc.createOffer());
      await gathered(pc);
      const answer = await json(await fetch(base, { method: "POST", body: JSON.stringify({ sdp: pc.localDescription!.sdp, display: shown }) }));
      if (live.current?.pc !== pc) {
        // Stopped while connecting.
        fetch(`${base}/${answer.session}`, { method: "DELETE" }).catch(() => {});
        return;
      }
      live.current.session = answer.session;
      await pc.setRemoteDescription({ type: "answer", sdp: answer.sdp });
      setState("live");
    } catch (e) {
      stop();
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  /** Sends one input Action; requests go out one at a time so the Device sees them in order. */
  function act(action: object) {
    flushClick();
    queue.current = queue.current.then(async () => {
      const res = await fetch(`/api/devices/${encodeURIComponent(device)}/actions`, {
        method: "POST",
        body: JSON.stringify({ actions: [action], observe: "none" }),
      });
      if (!res.ok) setInputError((await res.json().catch(() => null))?.error?.message ?? res.statusText);
    }).catch((e) => setInputError(String(e)));
  }

  // A single click waits briefly in case it is the first half of a double click.
  function flushClick() {
    const c = click.current;
    clearTimeout(c.timer);
    const send = c.send;
    c.send = undefined;
    send?.();
  }

  /** Mouse position in the streamed display's real pixels (the video's native size): `native` Action points. */
  function at(e: MouseEvent<HTMLVideoElement>) {
    const v = e.currentTarget;
    const x = Math.min(v.videoWidth - 1, Math.floor((e.nativeEvent.offsetX * v.videoWidth) / v.clientWidth));
    const y = Math.min(v.videoHeight - 1, Math.floor((e.nativeEvent.offsetY * v.videoHeight) / v.clientHeight));
    return { display, x: Math.max(0, x), y: Math.max(0, y), native: true };
  }

  const control = holding && state === "live";
  const input = control
    ? {
        tabIndex: 0,
        onMouseDown: (e: MouseEvent<HTMLVideoElement>) => {
          if (e.button === 0) click.current.down = at(e);
        },
        onMouseUp: (e: MouseEvent<HTMLVideoElement>) => {
          const from = click.current.down;
          click.current.down = undefined;
          const to = at(e);
          if (e.button === 0 && from && Math.hypot(to.x - from.x, to.y - from.y) > 4) {
            click.current.dragged = true;
            act({ type: "drag", ...from, to });
          }
        },
        onClick: (e: MouseEvent<HTMLVideoElement>) => {
          if (click.current.dragged) return void (click.current.dragged = false);
          if (e.detail > 1) return;
          const p = at(e);
          flushClick();
          click.current.send = () => act({ type: "click", ...p });
          click.current.timer = setTimeout(flushClick, 250);
        },
        onDoubleClick: (e: MouseEvent<HTMLVideoElement>) => {
          clearTimeout(click.current.timer);
          click.current.send = undefined;
          act({ type: "double_click", ...at(e) });
        },
        onContextMenu: (e: MouseEvent<HTMLVideoElement>) => {
          e.preventDefault();
          act({ type: "right_click", ...at(e) });
        },
        onWheel: (e: WheelEvent<HTMLVideoElement>) => {
          // One wheel notch is about 100 px of deltaY.
          const notches = (d: number) => (d === 0 ? 0 : Math.sign(d) * Math.max(1, Math.round(Math.abs(d) / 100)));
          act({ type: "scroll", ...at(e), dx: notches(e.deltaX), dy: notches(e.deltaY) });
        },
        onKeyDown: (e: KeyboardEvent<HTMLVideoElement>) => {
          if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
          e.preventDefault();
          const plain = e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey;
          // Any keystroke may be part of a password: `secret` keeps typed text out of the audit log.
          act(plain ? { type: "type", text: e.key, secret: true } : { type: "key", keys: keys(e) });
        },
      }
    : {};

  useEffect(() => {
    if (video.current) video.current.srcObject = stream ?? null;
  }, [stream, state]);
  // Leaving the page ends the stream (the Agent's control indicator turns off).
  useEffect(() => () => stop(), []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <Card aria-label="Live view" role="region" className="gap-3">
      <CardHeader className="flex flex-row items-center justify-between gap-3">
        <CardTitle className="text-base">Live view</CardTitle>
        {state === "idle" ? (
          <Button size="sm" onClick={() => start()}>
            Xem trực tiếp
          </Button>
        ) : (
          <span className="flex items-center gap-2">
            {displays.length > 1 && (
              <select
                aria-label="Chọn màn hình"
                className={nativeSelect}
                value={display}
                onChange={(e) => {
                  stop();
                  start(Number(e.target.value));
                }}
              >
                {displays.map((d) => (
                  <option key={d.index} value={d.index}>
                    Màn hình {d.index + 1} ({d.width}×{d.height})
                  </option>
                ))}
              </select>
            )}
            <select
              ref={quality}
              aria-label="Live View Quality"
              title="Quality: nét nhất. Balanced. Speed: mượt nhất."
              className={nativeSelect}
              defaultValue="balanced"
              onChange={(e) => {
                const picker = live.current?.picker;
                if (picker?.readyState === "open") picker.send(e.target.value);
              }}
            >
              <option value="quality">Quality</option>
              <option value="balanced">Balanced</option>
              <option value="speed">Speed</option>
            </select>
            <Button size="sm" variant="secondary" onClick={stop}>
              Dừng xem
            </Button>
          </span>
        )}
      </CardHeader>
      <CardContent className="grid gap-2">
        {error && <p className="text-sm text-muted-foreground">Không xem được: {error}</p>}
        {state === "connecting" && <p className="text-sm text-muted-foreground">Đang kết nối…</p>}
        {state === "live" && !holding && <p className="text-sm text-muted-foreground">Chỉ xem: cần giữ Control Lock để điều khiển.</p>}
        {control && inputError && <p className="text-sm text-muted-foreground">Không điều khiển được: {inputError}</p>}
        {state === "idle" && (
          <div className="grid aspect-video place-items-center bg-ink text-sm font-medium text-paper-2">Chưa xem màn hình</div>
        )}
        {state !== "idle" && (
          <video
            ref={video}
            aria-label={`Màn hình ${name}`}
            autoPlay
            muted
            playsInline
            className="w-full bg-ink"
            style={{ cursor: control ? "crosshair" : undefined }}
            {...input}
          />
        )}
      </CardContent>
    </Card>
  );
}
