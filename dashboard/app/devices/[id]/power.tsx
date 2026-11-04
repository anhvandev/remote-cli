"use client";
// Wake and power buttons: the same Controller API operations as `rcli wake` / `rcli power`. The Server enforces the
// rules; the buttons only mirror them (Restart/Shut down/Sleep need the Control Lock free, held by me, or admin).
import { useRouter } from "next/navigation";
import { useState } from "react";
import { power, wake, type Sent } from "../../actions";
import type { DeviceStatus } from "@/lib/api";
import { useLock } from "./control";

const ACTIONS = [
  { action: "lock", label: "Lock screen" },
  { action: "sleep", label: "Sleep" },
  { action: "restart", label: "Restart", confirm: true },
  { action: "shutdown", label: "Shut down", confirm: true },
] as const;

export function PowerButtons(props: { device: string; name: string; status: DeviceStatus; me: string; admin: boolean }) {
  const lock = useLock();
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ label: string } & Sent>();
  const blocked = !!lock.holder && lock.holder !== props.me && !props.admin;
  const online = props.status === "Online";

  async function send(label: string, run: () => Promise<Sent>) {
    setBusy(true);
    try {
      setMsg({ label, ...(await run()) });
      router.refresh();
    } finally {
      setBusy(false);
    }
  }

  const why = blocked
    ? `Control Lock đang do ${lock.holder} giữ${lock.expires_at ? ` đến ${new Date(lock.expires_at).toLocaleTimeString("vi-VN")}` : ""}`
    : undefined;
  return (
    <section aria-label="Nguồn">
      <div className="row">
        <button className="btn" disabled={online || busy} onClick={() => send("Wake", () => wake(props.device))}>
          Wake
        </button>
        {online &&
          ACTIONS.map((a) => (
            <button
              key={a.action}
              className={a.action === "shutdown" ? "btn danger" : "btn"}
              disabled={busy || (a.action !== "lock" && blocked)}
              title={a.action !== "lock" ? why : undefined}
              onClick={() => {
                if ("confirm" in a && !confirm(`${a.label} ${props.name}?`)) return;
                send(a.label, () => power(props.device, a.action));
              }}
            >
              {a.label}
            </button>
          ))}
      </div>
      {online && why && <p className="muted">{why}: chỉ Lock screen được phép.</p>}
      {msg && !msg.error && <p className="muted">Đã gửi {msg.label}{msg.detail && ` ${msg.detail}`}.</p>}
      {msg?.error && (
        <p role="alert" className="muted">
          {msg.label} bị từ chối: {msg.error}
        </p>
      )}
    </section>
  );
}
