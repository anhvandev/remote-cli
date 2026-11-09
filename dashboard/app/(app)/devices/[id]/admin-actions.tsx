"use client";
// Admin-only actions on a Device's page: change the Device ID, rename it, and set a new Device Password.
// The Server pushes the ID and password to the Device (which must be Online); the current password is never shown.
import { useRouter } from "next/navigation";
import { useState } from "react";
import { changeDevice, type Sent } from "../../../actions";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { card, cardHead, title } from "../../styles";

type Field = "name" | "device_id" | "device_password";

export function AdminActions({ device, deviceId }: { device: string; deviceId: string }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ label: string; done: string } & Sent>();

  async function send(label: string, done: string, change: Partial<Record<Field, string>>, form: HTMLFormElement) {
    setBusy(true);
    try {
      const result = await changeDevice(device, change);
      setMsg({ label, done, ...result });
      if (!result.error) {
        form.reset();
        router.refresh();
      }
    } finally {
      setBusy(false);
    }
  }

  const submit = (label: string, done: string, field: Field) => (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const form = e.currentTarget;
    const value = String(new FormData(form).get(field) ?? "");
    send(label, done, { [field]: value }, form);
  };

  return (
    <section aria-label="Quản trị Device" className={card}>
      <div className={cardHead}>
        <h3 className={title}>Quản trị Device</h3>
        <span className="font-mono text-xs text-muted-foreground">Device ID {deviceId}</span>
      </div>
      <div className="grid gap-4 p-5 text-sm">
        <p className="text-xs text-muted-foreground">
          Device ID và mật khẩu mới được gửi tới Device ngay, nên Device phải Online. Admin không xem được mật khẩu hiện
          tại.
        </p>
        <form onSubmit={submit("Đổi Device ID", "Đã đổi Device ID", "device_id")} className="flex flex-wrap items-center gap-2">
          <Input
            name="device_id"
            aria-label="Device ID mới"
            placeholder="9 chữ số"
            inputMode="numeric"
            pattern="[0-9]{9}"
            maxLength={9}
            required
            className="h-8 w-36 font-mono"
          />
          <Button size="sm" variant="outline" disabled={busy}>
            Đổi Device ID
          </Button>
        </form>
        <form onSubmit={submit("Đổi tên", "Đã đổi tên", "name")} className="flex flex-wrap items-center gap-2">
          <Input name="name" aria-label="Tên mới" placeholder="tên mới" maxLength={255} required className="h-8 w-36" />
          <Button size="sm" variant="outline" disabled={busy}>
            Đổi tên
          </Button>
        </form>
        <form onSubmit={submit("Đặt mật khẩu", "Đã đặt mật khẩu mới", "device_password")} className="flex flex-wrap items-center gap-2">
          <Input
            type="password"
            name="device_password"
            aria-label="Device Password mới"
            placeholder="ít nhất 8 ký tự"
            autoComplete="new-password"
            minLength={8}
            required
            className="h-8 w-36"
          />
          <Button size="sm" variant="outline" disabled={busy}>
            Đặt mật khẩu
          </Button>
        </form>
        {msg && !msg.error && <output role="status" className="text-xs text-muted-foreground">{msg.done}.</output>}
        {msg?.error && (
          <span role="alert" className="text-sm text-destructive">
            {msg.label} bị từ chối: {msg.error}
          </span>
        )}
      </div>
    </section>
  );
}
