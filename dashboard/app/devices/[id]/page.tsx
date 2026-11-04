import { notFound } from "next/navigation";
import { api, ApiError, type ControlLock, type DeviceDetails, type Resources, type User } from "@/lib/api";
import { Nav } from "../../nav";
import { Refresh } from "../../refresh";
import { Status } from "../../status";
import { History } from "./history";
import { LiveView } from "./live-view";
import { Terminal } from "./terminal";

const GB = 1 << 30;
const gb = (b: number) => `${(b / GB).toFixed(1)} GB`;
const rate = (b?: number) => (b === undefined ? "—" : `${(b / 1e6).toFixed(1)} MB/s`);
const pct = (p?: number) => (p === undefined ? "—" : `${Math.round(p)}%`);
const uptime = (s: number) => `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`;

function Screen({ r }: { r: Resources }) {
  switch (r.session) {
    case "Active":
      return <span className="tag">Đã đăng nhập · {r.logged_in_user || "?"}</span>;
    case "LockScreen":
      return <span className="tag">Màn khoá · {r.logged_in_user || "?"}</span>;
    case "LoginScreen":
      return <span className="tag warn">Màn đăng nhập</span>;
    default:
      return <span className="muted">—</span>;
  }
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
  const [d, lock] = await Promise.all([api<DeviceDetails>(path), api<ControlLock>(`${path}/lock`)]).catch((e) => {
    if (e instanceof ApiError && e.status === 404) notFound();
    throw e;
  });
  const me = await api<User>("/api/v1/me");
  const time = (t: string) => new Date(t).toLocaleTimeString("vi-VN");
  const inv = d.inventory;
  const r = d.resources;
  return (
    <>
      <Nav />
      <Refresh />
      <main>
        <h2>
          {d.name}{" "}
          <span style={{ fontSize: 14, fontWeight: 400 }}>
            <Status status={d.status} />
          </span>
        </h2>
        <div className="grid" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", alignItems: "start" }}>
          <section className="card" aria-label="Control Lock">
            <h3>Control Lock</h3>
            {lock.holder ? (
              <div className="row">
                <span className="tag lock">🔒 {lock.holder}</span>
                <span className="muted">
                  {lock.expires_at ? (
                    <>hết hạn lúc <b className="mono">{time(lock.expires_at)}</b></>
                  ) : (
                    lock.acquired_at && <>giữ từ <b className="mono">{time(lock.acquired_at)}</b></>
                  )}
                </span>
              </div>
            ) : (
              <span className="muted">Không ai giữ</span>
            )}
          </section>

          <section className="card" aria-label="Tài nguyên">
            <h3>Tài nguyên</h3>
            {r ? (
              <div className="grid" style={{ gridTemplateColumns: "1fr 1fr" }}>
                <div className="kpi" style={{ gridColumn: "1/-1" }}>
                  <span className="muted">Màn hình</span> <Screen r={r} />
                </div>
                <Kpi label="CPU" value={pct(r.cpu_percent)} />
                <Kpi
                  label="RAM"
                  value={r.ram_used_bytes === undefined ? "—" : inv ? `${gb(r.ram_used_bytes)} / ${gb(inv.ram_bytes)}` : gb(r.ram_used_bytes)}
                />
                <Kpi label="GPU" value={pct(r.gpu_percent)} />
                {Object.entries(r.disk_used_percent).map(([mount, p]) => (
                  <Kpi key={mount} label={`Disk ${mount}`} value={pct(p)} />
                ))}
                <Kpi label="Disk đọc/ghi" value={`${rate(r.disk_read_bytes_per_sec)} / ${rate(r.disk_write_bytes_per_sec)}`} />
                <Kpi label="Mạng ↑/↓" value={`${rate(r.net_up_bytes_per_sec)} / ${rate(r.net_down_bytes_per_sec)}`} />
                {r.uptime_secs !== undefined && <Kpi label="Uptime" value={uptime(r.uptime_secs)} />}
                {r.cpu_temp_celsius !== undefined && <Kpi label="CPU temp" value={`${Math.round(r.cpu_temp_celsius)}°C`} />}
                {r.battery_percent !== undefined && (
                  <Kpi label="Pin" value={`${pct(r.battery_percent)}${r.battery_charging ? " ⚡" : ""}`} />
                )}
              </div>
            ) : (
              <span className="muted">Chỉ có khi Device Online.</span>
            )}
          </section>

          <LiveView device={d.id} name={d.name} />
          <Terminal device={d.id} admin={me.role === "admin"} />

          <section className="card" aria-label="Phần cứng">
            <h3>Phần cứng</h3>
            {inv ? (
              <table className="muted">
                <tbody>
                  <Row k="OS" v={`${inv.os} ${inv.os_version}`} />
                  <Row k="Hostname" v={inv.hostname} />
                  <Row k="CPU" v={`${inv.cpu_model} · ${inv.cpu_cores} nhân`} />
                  <Row k="RAM" v={gb(inv.ram_bytes)} />
                  {inv.gpus.map((g, i) => <Row key={i} k="GPU" v={g} />)}
                  {inv.disks.map((x, i) => <Row key={i} k="Disk" v={`${x.name} ${x.mount} · ${gb(x.total_bytes)}`} />)}
                  {inv.network_interfaces.map((n, i) => (
                    <Row key={i} k="Mạng" v={`${n.name} · ${n.mac} · ${n.ips.join(", ")}`} />
                  ))}
                  {inv.displays.map((x, i) => <Row key={i} k="Màn hình" v={`${x.name} · ${x.width}×${x.height}`} />)}
                  <Row k="Serial" v={inv.serial} />
                </tbody>
              </table>
            ) : (
              <span className="muted">Agent chưa gửi thông tin phần cứng.</span>
            )}
          </section>
        </div>
        <div style={{ marginTop: 12 }}>
          <History path={path} range={range} ramBytes={inv?.ram_bytes} />
        </div>
      </main>
    </>
  );
}

function Kpi({ label, value }: { label: string; value: string }) {
  return (
    <div className="kpi">
      <span className="muted">{label}</span>
      <b>{value}</b>
    </div>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <tr>
      <td>{k}</td>
      <td>{v || "—"}</td>
    </tr>
  );
}
