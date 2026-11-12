"use client";
// A Devices page row's ⋯ menu: every Device action (rename, Device ID, Device Password, merge, revoke, delete).
// The Server pushes a new Device ID or password to the Device (it must be Online) and never returns a password.
import { Popover } from "radix-ui";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { Device } from "@/lib/api";
import * as act from "./admin/actions";
import { Form } from "./admin/form";
import { nativeSelect } from "./styles";

export function DeviceMenu({ device: d, others }: { device: Device; others: Device[] }) {
  const label = `Thao tác cho ${d.name}`;
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <Button size="xs" variant="ghost" aria-label={label} className="relative z-10">
          ⋯
        </Button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          aria-label={label}
          align="end"
          className="z-50 grid w-80 gap-3 rounded-lg border border-border bg-popover p-4 text-sm text-popover-foreground shadow-md"
        >
          <Form action={act.updateDevice}>
            <input type="hidden" name="device" value={d.id} />
            <Input name="name" aria-label="Tên mới" placeholder="tên mới" maxLength={255} required className="h-8 w-36" />
            <Button size="xs" variant="outline">
              Đổi tên
            </Button>
          </Form>
          <Form action={act.updateDevice}>
            <input type="hidden" name="device" value={d.id} />
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
            <Button size="xs" variant="outline">
              Đổi Device ID
            </Button>
          </Form>
          <Form action={act.updateDevice}>
            <input type="hidden" name="device" value={d.id} />
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
            <Button size="xs" variant="outline">
              Đặt mật khẩu
            </Button>
          </Form>
          <Form action={act.mergeDevice} confirm={`Gộp lịch sử vào ${d.name} rồi xoá Device cũ?`}>
            <input type="hidden" name="device" value={d.id} />
            <select
              name="from"
              aria-label={`Gộp lịch sử từ Device vào ${d.name}`}
              required
              defaultValue=""
              className={nativeSelect}
            >
              <option value="" disabled>
                Gộp lịch sử từ…
              </option>
              {others.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.name}
                </option>
              ))}
            </select>
            <Button size="xs" variant="outline">
              Gộp
            </Button>
          </Form>
          <div className="flex gap-2">
            {!d.revoked_at && (
              <Form action={act.revokeDevice} confirm={`Thu hồi ${d.name}? Device không thể kết nối lại.`}>
                <input type="hidden" name="device" value={d.id} />
                <Button size="xs" variant="ghost" className="text-destructive">
                  Thu hồi
                </Button>
              </Form>
            )}
            <Form action={act.deleteDevice} confirm={`Xoá ${d.name} cùng lịch sử của nó? Device sẽ bị ngắt kết nối.`}>
              <input type="hidden" name="device" value={d.id} />
              <Button size="xs" variant="ghost" className="text-destructive">
                Xoá
              </Button>
            </Form>
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
