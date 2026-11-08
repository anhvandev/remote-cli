"use client";
// Wake and power buttons: the same Controller API operations as `rcli wake` / `rcli power`. The Server enforces the
// rules; the buttons only mirror them (Restart/Shut down/Sleep need the Control Lock free, held by me, or admin).
import { useRouter } from "next/navigation";
import { useState } from "react";
import { power, wake, type Sent } from "../../../actions";
import type { DeviceStatus } from "@/lib/api";
import { Button } from "@/components/ui/button";
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
    <section aria-label="Nguồn" className="flex flex-wrap items-center gap-2">
      <Button variant="outline" size="sm" disabled={online || busy} onClick={() => send("Wake", () => wake(props.device))}>
        Wake
      </Button>
      {online &&
        ACTIONS.map((a) => (
          <Button
            key={a.action}
            size="sm"
            variant={a.action === "shutdown" ? "outline" : "secondary"}
            className={a.action === "shutdown" ? "text-destructive hover:text-destructive" : undefined}
            disabled={busy || (a.action !== "lock" && blocked)}
            title={a.action !== "lock" ? why : undefined}
            onClick={() => {
              if ("confirm" in a && !confirm(`${a.label} ${props.name}?`)) return;
              send(a.label, () => power(props.device, a.action));
            }}
          >
            {a.label}
          </Button>
        ))}
      {(online && why) || msg ? (
        <p className="basis-full text-xs text-muted-foreground">
          {msg && !msg.error && <>Đã gửi {msg.label}{msg.detail && ` ${msg.detail}`}.</>}
          {msg?.error && (
            <span role="alert">
              {msg.label} bị từ chối: {msg.error}
            </span>
          )}
          {online && why && <> {why}: chỉ Lock screen được phép.</>}
        </p>
      ) : null}
    </section>
  );
}
