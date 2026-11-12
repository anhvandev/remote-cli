import Link from "next/link";
import { MonitorSmartphone } from "lucide-react";
import { api, type Device, type DeviceStatus } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Refresh } from "../refresh";
import { Status } from "../status";
import { DeviceMenu } from "./device-menu";
import { FleetFilter } from "./fleet-filter";

/** "2 phút trước" — easier to scan than an absolute clock. */
function ago(t: string | null) {
  if (!t) return "—";
  const s = Math.max(0, (Date.now() - new Date(t).getTime()) / 1000);
  if (s < 60) return "vừa xong";
  if (s < 3600) return `${Math.floor(s / 60)} phút trước`;
  if (s < 86400) return `${Math.floor(s / 3600)} giờ trước`;
  return `${Math.floor(s / 86400)} ngày trước`;
}

export default async function FleetPage({
  searchParams,
}: {
  searchParams: Promise<{ q?: string; status?: DeviceStatus }>;
}) {
  const devices = await api<Device[]>("/api/v1/devices");
  const { q, status } = await searchParams;
  const name = q?.toLowerCase() ?? "";
  const shown = devices.filter((d) => d.name.toLowerCase().includes(name) && (!status || d.status === status));
  const count = (s: DeviceStatus) => devices.filter((d) => d.status === s).length;
  return (
    <>
      <Refresh />
      <h2 className="mb-4 flex flex-wrap items-baseline gap-x-3 gap-y-1 text-xl font-semibold tracking-tight">
        Device
        <span className="text-sm font-normal text-muted-foreground">
          · {count("Online")} Online · {count("Offline")} Offline · {count("Waking")} Waking
        </span>
      </h2>
      <FleetFilter />
      <div className="overflow-hidden rounded-xl border border-border bg-card">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Device</TableHead>
              <TableHead>Trạng thái</TableHead>
              <TableHead>Agent</TableHead>
              <TableHead>Lần cuối thấy</TableHead>
              <TableHead />
            </TableRow>
          </TableHeader>
          <TableBody>
            {shown.map((d) => (
              <TableRow key={d.id} className="relative cursor-pointer">
                <TableCell>
                  <Link href={`/devices/${d.id}`} className="font-medium after:absolute after:inset-0 after:content-['']">
                    {d.name}
                  </Link>
                  <span className="ml-2 font-mono text-xs text-muted-foreground">{d.device_id}</span>
                  {d.revoked_at && (
                    <Badge variant="outline" className="ml-2 border-warn/40 text-warn">
                      đã thu hồi
                    </Badge>
                  )}
                </TableCell>
                <TableCell>
                  <Status status={d.status} wakeError={d.wake_error} />
                </TableCell>
                <TableCell className="text-muted-foreground">
                  {d.agent_version ?? "—"}
                  {d.update && d.update.status !== "updated" && (
                    <span className={d.update.status === "failed" ? "text-destructive" : ""}>
                      {" → "}
                      {d.update.version} · {d.update.status}
                    </span>
                  )}
                </TableCell>
                <TableCell className="whitespace-nowrap text-muted-foreground">{ago(d.last_seen_at)}</TableCell>
                <TableCell className="w-0 text-right">
                  <DeviceMenu device={d} others={devices.filter((o) => o.id !== d.id)} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        {(devices.length === 0 || shown.length === 0) && (
          <div className="flex flex-col items-center gap-2 px-6 py-14 text-center text-sm text-muted-foreground">
            <MonitorSmartphone className="size-6 opacity-60" />
            {devices.length === 0 ? "Chưa có Device nào." : "Không có Device nào khớp."}
          </div>
        )}
      </div>
    </>
  );
}
