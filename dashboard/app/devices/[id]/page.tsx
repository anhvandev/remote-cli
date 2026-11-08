import { notFound } from "next/navigation";
import { api, ApiError, type ControlLock, type DeviceDetails, type Resources, type User } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Nav } from "../../nav";
import { Refresh } from "../../refresh";
import { Status } from "../../status";
import { History } from "./history";
import { ControlProvider, LockCard } from "./control";
import { LiveView } from "./live-view";
import { PowerButtons } from "./power";
import { Terminal } from "./terminal";

const GB = 1 << 30;
const gb = (b: number) => `${(b / GB).toFixed(1)} GB`;
const rate = (b?: number) => (b === undefined ? "—" : `${(b / 1e6).toFixed(1)} MB/s`);
const pct = (p?: number) => (p === undefined ? "—" : `${Math.round(p)}%`);
const uptime = (s: number) => `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`;

function Screen({ r }: { r: Resources }) {
  switch (r.session) {
    case "Active":
      return <span className="rounded-full bg-secondary px-2 py-0.5 text-xs">Đã đăng nhập · {r.logged_in_user || "?"}</span>;
    case "LockScreen":
      return <span className="rounded-full bg-secondary px-2 py-0.5 text-xs">Màn khoá · {r.logged_in_user || "?"}</span>;
    case "LoginScreen":
      return <span className="rounded-full border border-warn/35 bg-warn/10 px-2 py-0.5 text-xs text-warn">Màn đăng nhập</span>;
    default:
      return <span className="text-sm text-muted-foreground">—</span>;
  }
}

function Meter({ value }: { value?: number }) {
  const w = value === undefined ? 0 : Math.min(100, Math.max(0, value));
  const tone = w >= 90 ? "bg-bad" : w >= 70 ? "bg-warn" : "bg-primary";
  return (
    <span className="inline-flex items-center gap-2">
      <span className="h-1.5 w-16 overflow-hidden rounded-full bg-border">
        <span className={`block h-full rounded-full ${tone}`} style={{ width: `${w}%` }} />
      </span>
      <b className="text-base font-semibold tabular-nums">{pct(value)}</b>
    </span>
  );
}

export default async function DevicePage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ range?: string }>;
}) {
  const { id } = await params;
  const { range } = await searchParams;
  const path = `/api/v1/devices/${encodeURIComponent(id)}`;
  const [d, lock, me] = await Promise.all([
    api<DeviceDetails>(path),
    api<ControlLock>(`${path}/lock`),
    api<User>("/api/v1/me"),
  ]).catch((e) => {
    if (e instanceof ApiError && e.status === 404) notFound();
    throw e;
  });
  const inv = d.inventory;
  const r = d.resources;
  // Header meta line: OS, hostname/IP, agent version, uptime, signed-in user.
  const ip = inv?.network_interfaces.flatMap((n) => n.ips)[0];
  const meta = [
    `${inv?.os} ${inv?.os_version}`.trim(),
    ip,
    d.agent_version && `Agent ${d.agent_version}`,
    r?.uptime_secs !== undefined && `uptime ${uptime(r.uptime_secs)}`,
    r?.logged_in_user && `user: ${r.logged_in_user}`,
  ].filter(Boolean);

  return (
    <>
      <Nav />
      <Refresh />
      <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
        <div className="mx-auto max-w-6xl">
          <ControlProvider device={d.id} me={me.name} initial={lock}>
            <div className="flex flex-wrap items-start justify-between gap-4 border-b border-border pb-5">
              <div className="min-w-0">
                <h2 className="flex flex-wrap items-center gap-2.5 text-xl font-semibold tracking-tight">
                  {d.name}
                  <span className="text-sm font-normal">
                    <Status status={d.status} wakeError={d.wake_error} />
                  </span>
                </h2>
                <p className="mt-1 truncate text-sm text-muted-foreground">{meta.join(" · ")}</p>
              </div>
              <PowerButtons device={d.id} name={d.name} status={d.status} me={me.name} admin={me.role === "admin"} />
            </div>

            <div className="mt-5 grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
              <div className="grid min-w-0 gap-5">
                <LiveView device={d.id} name={d.name} />
                <Terminal device={d.id} admin={me.role === "admin"} />
              </div>

              <div className="grid gap-5">
                <LockCard />

                <section className="rounded-xl border border-border bg-card" aria-label="Tài nguyên">
                  <div className="flex items-center justify-between border-b border-border px-5 py-3.5">
                    <h3 className="text-sm font-medium text-muted-foreground">Tài nguyên</h3>
                    {r && <Screen r={r} />}
                  </div>
                  <div className="p-5">
                    {r ? (
                      <div className="grid grid-cols-2 gap-x-4 gap-y-4">
                        <Kpi label="CPU">
                          <Meter value={r.cpu_percent} />
                        </Kpi>
                        <Kpi label="GPU">
                          <Meter value={r.gpu_percent} />
                        </Kpi>
                        <Kpi label="RAM">
                          <b className="text-base font-semibold tabular-nums">
                            {r.ram_used_bytes === undefined ? "—" : inv ? `${gb(r.ram_used_bytes)}` : gb(r.ram_used_bytes)}
                          </b>
                          {inv && r.ram_used_bytes !== undefined && <span className="text-xs text-muted-foreground"> / {gb(inv.ram_bytes)}</span>}
                        </Kpi>
                        <Kpi label="Disk đọc/ghi" wide>
                          <b className="font-mono text-sm font-medium">
                            {rate(r.disk_read_bytes_per_sec)} / {rate(r.disk_write_bytes_per_sec)}
                          </b>
                        </Kpi>
                        <Kpi label="Mạng ↑/↓" wide>
                          <b className="font-mono text-sm font-medium">
                            {rate(r.net_up_bytes_per_sec)} / {rate(r.net_down_bytes_per_sec)}
                          </b>
                        </Kpi>
                        {Object.entries(r.disk_used_percent).map(([mount, p]) => (
                          <Kpi key={mount} label={`Disk ${mount}`}>
                            <Meter value={p} />
                          </Kpi>
                        ))}
                        {r.uptime_secs !== undefined && (
                          <Kpi label="Uptime">
                            <b className="text-base font-semibold tabular-nums">{uptime(r.uptime_secs)}</b>
                          </Kpi>
                        )}
                        {r.cpu_temp_celsius !== undefined && (
                          <Kpi label="CPU temp">
                            <b className="text-base font-semibold tabular-nums">{Math.round(r.cpu_temp_celsius)}°C</b>
                          </Kpi>
                        )}
                        {r.battery_percent !== undefined && (
                          <Kpi label="Pin">
                            <b className="text-base font-semibold tabular-nums">
                              {pct(r.battery_percent)}
                              {r.battery_charging ? " ⚡" : ""}
                            </b>
                          </Kpi>
                        )}
                      </div>
                    ) : (
                      <span className="text-sm text-muted-foreground">Chỉ có khi Device Online.</span>
                    )}
                  </div>
                </section>

                <section className="rounded-xl border border-border bg-card" aria-label="Phần cứng">
                  <div className="border-b border-border px-5 py-3.5">
                    <h3 className="text-sm font-medium text-muted-foreground">Phần cứng</h3>
                  </div>
                  <div className="p-5">
                    {inv ? (
                      <table className="w-full text-sm">
                        <tbody>
                          <Row k="OS" v={`${inv.os} ${inv.os_version}`} />
                          <Row k="Hostname" v={inv.hostname} />
                          <Row k="CPU" v={`${inv.cpu_model} · ${inv.cpu_cores} nhân`} />
                          <Row k="RAM" v={gb(inv.ram_bytes)} />
                          {inv.gpus.map((g, i) => (
                            <Row key={i} k="GPU" v={g} />
                          ))}
                          {inv.disks.map((x, i) => (
                            <Row key={i} k="Disk" v={`${x.name} ${x.mount} · ${gb(x.total_bytes)}`} />
                          ))}
                          {inv.network_interfaces.map((n, i) => (
                            <Row key={i} k="Mạng" v={`${n.name} · ${n.mac} · ${n.ips.join(", ")}`} />
                          ))}
                          {inv.displays.map((x, i) => (
                            <Row key={i} k="Màn hình" v={`${x.name} · ${x.width}×${x.height}`} />
                          ))}
                          <Row k="Serial" v={inv.serial} mono />
                        </tbody>
                      </table>
                    ) : (
                      <span className="text-sm text-muted-foreground">Agent chưa gửi thông tin phần cứng.</span>
                    )}
                  </div>
                </section>
              </div>
            </div>
          </ControlProvider>
          <div className="mt-5">
            <History path={path} range={range} ramBytes={inv?.ram_bytes} />
          </div>
        </div>
      </main>
    </>
  );
}

function Kpi({ label, children, wide }: { label: string; children: React.ReactNode; wide?: boolean }) {
  return (
    <div className={wide ? "col-span-2" : undefined}>
      <div className="mb-1 text-xs text-muted-foreground">{label}</div>
      {children}
    </div>
  );
}

function Row({ k, v, mono }: { k: string; v: string; mono?: boolean }) {
  return (
    <tr className="border-b border-border/60 last:border-0">
      <td className="py-1.5 pr-3 align-top text-muted-foreground">{k}</td>
      <td className={`py-1.5 text-right align-top break-words ${mono ? "font-mono text-xs" : ""}`}>{v || "—"}</td>
    </tr>
  );
}
