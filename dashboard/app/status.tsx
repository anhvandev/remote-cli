import type { DeviceStatus } from "@/lib/api";

export function Status({ status }: { status: DeviceStatus }) {
  return (
    <span className={status}>
      <span className="dot" />
      {status}
    </span>
  );
}
