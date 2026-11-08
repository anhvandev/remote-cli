import Link from "next/link";
import { MonitorSmartphone } from "lucide-react";
import { api, type Device } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Refresh } from "../refresh";
import { Status } from "../status";

/** "2 phút trước" — easier to scan than an absolute clock. */
function ago(t: string | null) {
  if (!t) return "—";
  const s = Math.max(0, (Date.now() - new Date(t).getTime()) / 1000);
  if (s < 60) return "vừa xong";
  if (s < 3600) return `${Math.floor(s / 60)} phút trước`;
  if (s < 86400) return `${Math.floor(s / 3600)} giờ trước`;
  return `${Math.floor(s / 86400)} ngày trước`;
}

export default async function FleetPage() {
  const devices = await api<Device[]>("/api/v1/devices");
  const count = (s: string) => devices.filter((d) => d.status === s).length;
  return (
    <>
      <Refresh />
      <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
        <div className="mx-auto max-w-6xl">
          <h2 className="mb-4 flex flex-wrap items-baseline gap-x-3 gap-y-1 text-xl font-semibold tracking-tight">
            Device
            <span className="text-sm font-normal text-muted-foreground">
              · {count("Online")} Online · {count("Offline")} Offline · {count("Waking")} Waking
            </span>
          </h2>
          <div className="overflow-hidden rounded-xl border border-border bg-card">
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>Device</TableHead>
                  <TableHead>Trạng thái</TableHead>
                  <TableHead>Agent</TableHead>
                  <TableHead>Lần cuối thấy</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {devices.map((d) => (
                  <TableRow key={d.id} className="relative cursor-pointer">
                    <TableCell>
                      <Link href={`/devices/${d.id}`} className="font-medium after:absolute after:inset-0 after:content-['']">
                        {d.name}
                      </Link>
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
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {devices.length === 0 && (
              <div className="flex flex-col items-center gap-2 px-6 py-14 text-center text-sm text-muted-foreground">
                <MonitorSmartphone className="size-6 opacity-60" />
                Chưa có Device nào.
              </div>
            )}
          </div>
        </div>
      </main>
    </>
  );
}
