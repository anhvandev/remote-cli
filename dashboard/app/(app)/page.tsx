import Link from "next/link";
import { api, type Device, type DeviceStatus, type ServerSettings } from "@/lib/api";
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
  const [devices, settings] = await Promise.all([
    api<Device[]>("/api/v1/devices"),
    api<ServerSettings>("/api/v1/server-settings"),
  ]);
  const { q, status } = await searchParams;
  const name = q?.toLowerCase() ?? "";
  const shown = devices.filter((d) => d.name.toLowerCase().includes(name) && (!status || d.status === status));
  const count = (s: DeviceStatus) => devices.filter((d) => d.status === s).length;
  return (
    <>
      <Refresh />
      {/* The board's head: the counts are the figures an operator reads first. The hidden " · " keeps the
          heading's text "6 Online · 5 Offline · 0 Waking", which screen readers and the tests read. */}
      <h2 className="mb-5 flex max-w-none flex-wrap items-end justify-between gap-x-10 gap-y-4 border-b-2 border-ink pb-4 text-3xl font-extrabold tracking-tight">
        <span>Device</span>{" "}
        <span className="flex flex-wrap gap-x-8 gap-y-2 tracking-normal">
          {(["Online", "Offline", "Waking"] as const).map((s, i) => (
            <span key={s} className="flex items-baseline gap-2">
              {i > 0 && <span className="sr-only"> · </span>}
              <span className={s === "Online" ? "text-primary" : undefined}>{count(s)}</span>{" "}
              <span className="text-sm font-medium text-muted-foreground">{s}</span>
            </span>
          ))}
        </span>
      </h2>
      <FleetFilter />
      <div>
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
                  <Link href={`/devices/${d.id}`} className="font-extrabold after:absolute after:inset-0 after:content-[''] hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring">
                    {d.name}
                  </Link>
                  <span className="ml-3 text-xs text-muted-foreground">{d.device_id}</span>
                  {d.revoked_at && (
                    <Badge className="ml-2 bg-warn text-ink">
                      đã thu hồi
                    </Badge>
                  )}
                </TableCell>
                <TableCell>
                  <Status status={d.status} wakeError={d.wake_error} />
                </TableCell>
                <TableCell className="text-muted-foreground">
                  {d.agent_version ?? "—"}
                  {d.needs_update && (
                    <Badge className="ml-2 bg-warn text-ink">
                      update needed · {d.agent_version} &lt; {settings.minimum_version}
                    </Badge>
                  )}
                  {d.update_error && <div className="text-xs text-destructive">{d.update_error}</div>}
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
          <div className="flex flex-col items-start gap-2 border-b-2 border-ink px-3 py-10 text-sm text-muted-foreground">
            {devices.length === 0 ? "Chưa có Device nào." : "Không có Device nào khớp."}
          </div>
        )}
      </div>
    </>
  );
}
