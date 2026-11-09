import type { DeviceStatus } from "@/lib/api";
import { cn } from "@/lib/utils";

const TONE: Record<DeviceStatus, string> = {
  Online: "bg-ok shadow-[0_0_0_3px_rgba(62,207,142,0.15)]",
  Offline: "bg-muted-foreground/50",
  Waking: "bg-warn shadow-[0_0_0_3px_rgba(245,165,36,0.15)]",
};

export function Status({ status, wakeError }: { status: DeviceStatus; wakeError?: string | null }) {
  // One text node: tests assert "Offline (Wake failed)" as contiguous text.
  return (
    <span className="inline-flex items-center gap-2 whitespace-nowrap text-sm">
      <span className={cn("size-2 rounded-full", TONE[status])} />
      {`${status}${wakeError ? ` (${wakeError})` : ""}`}
    </span>
  );
}
