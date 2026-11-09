import type { Resources } from "@/lib/api";
import { card, cardHead, title } from "../../styles";
import { getDevice, uptime } from "./device-data";

const GB = 1 << 30;
const gb = (b: number) => `${(b / GB).toFixed(1)} GB`;
const rate = (b?: number) => (b === undefined ? "—" : `${(b / 1e6).toFixed(1)} MB/s`);
const pct = (p?: number) => (p === undefined ? "—" : `${Math.round(p)}%`);

function SessionBadge({ r }: { r: Resources }) {
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

export default async function OverviewPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const d = await getDevice(id);
  const inv = d.inventory;
  const r = d.resources;
  return (
    <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
      <section className={card} aria-label="Tài nguyên">
        <div className={cardHead}>
          <h3 className={title}>Tài nguyên</h3>
          {r && <SessionBadge r={r} />}
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
                <b className="text-base font-semibold tabular-nums">{r.ram_used_bytes === undefined ? "—" : gb(r.ram_used_bytes)}</b>
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

      <section className={card} aria-label="Phần cứng">
        <div className={cardHead}>
          <h3 className={title}>Phần cứng</h3>
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
