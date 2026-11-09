"use client";
// Màn hình and Terminal render this instead of their content while the Device is Offline or Waking.
import { useRouter } from "next/navigation";
import { useState } from "react";
import { wake } from "../../../actions";
import type { DeviceStatus } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";

export function Placeholder({ device, status }: { device: string; status: Exclude<DeviceStatus, "Online"> }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  async function send() {
    setBusy(true);
    try {
      const { error } = await wake(device);
      setError(error);
      router.refresh();
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card aria-label="Không xem được" role="region" className="gap-3">
      <CardContent className="grid place-items-center gap-3 py-16 text-center text-sm text-muted-foreground">
        {status === "Waking" ? (
          <p>Đang chờ Device Online…</p>
        ) : (
          <>
            <p>Device đang Offline.</p>
            <Button size="sm" disabled={busy} onClick={send}>
              Wake
            </Button>
            {error && <p role="alert">Wake bị từ chối: {error}</p>}
          </>
        )}
      </CardContent>
    </Card>
  );
}
