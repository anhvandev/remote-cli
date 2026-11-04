import type { DeviceStatus } from "@/lib/api";

export function Status({ status, wakeError }: { status: DeviceStatus; wakeError?: string | null }) {
  return (
    <span className={status}>
      <span className="dot" />
      {status}
      {wakeError && ` (${wakeError})`}
    </span>
  );
}
