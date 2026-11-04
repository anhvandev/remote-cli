"use client";
// Live view: the browser is a WebRTC peer of the Device's Agent. Signaling goes through this app's route handlers
// (app/api/...) to the Server; media flows peer-to-peer, or through TURN when NAT blocks a direct path.
import { useEffect, useRef, useState } from "react";

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

export function LiveView({ device, name }: { device: string; name: string }) {
  const [state, setState] = useState<"idle" | "connecting" | "live">("idle");
  const [error, setError] = useState<string>();
  const [stream, setStream] = useState<MediaStream>();
  const live = useRef<{ pc: RTCPeerConnection; session?: string }>(undefined);
  const video = useRef<HTMLVideoElement>(null);
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

  async function start() {
    setError(undefined);
    setState("connecting");
    try {
      const pc = new RTCPeerConnection({ iceServers: await json(await fetch("/api/ice-servers")) });
      live.current = { pc };
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
      const answer = await json(await fetch(base, { method: "POST", body: JSON.stringify({ sdp: pc.localDescription!.sdp }) }));
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

  useEffect(() => {
    if (video.current) video.current.srcObject = stream ?? null;
  }, [stream, state]);
  // Leaving the page ends the stream (the Agent's control indicator turns off).
  useEffect(() => () => stop(), []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <section className="card" aria-label="Live view" style={{ gridColumn: "1/-1" }}>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3>Live view</h3>
        {state === "idle" ? (
          <button className="btn primary" onClick={start}>
            Xem trực tiếp
          </button>
        ) : (
          <button className="btn" onClick={stop}>
            Dừng xem
          </button>
        )}
      </div>
      {error && <p className="muted">Không xem được: {error}</p>}
      {state === "connecting" && <p className="muted">Đang kết nối…</p>}
      {state !== "idle" && (
        <video ref={video} aria-label={`Màn hình ${name}`} autoPlay muted playsInline style={{ width: "100%", background: "#000" }} />
      )}
    </section>
  );
}
