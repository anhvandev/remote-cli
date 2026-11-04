import Link from "next/link";
import { api, type History as HistoryData, type Resources } from "@/lib/api";

// Raw samples (720 points/hour) only for the short range; longer ranges read the 1-minute rollups.
const RANGES = { "1h": [1, "5s"], "24h": [24, "1m"], "7d": [24 * 7, "1m"], "90d": [24 * 90, "1m"] } as const;
type Range = keyof typeof RANGES;
const MAX_POINTS = 300;
const COLORS = ["#5b8cff", "#f0a35b", "#5bd08c", "#c4b5fd"];

const GB = 1 << 30;
const pct = (p: number) => `${Math.round(p)}%`;
const gb = (b: number) => `${(b / GB).toFixed(1)} GB`;
const rate = (b: number) => `${(b / 1e6).toFixed(1)} MB/s`;

type Series = { name: string; values: (number | undefined)[] };

export async function History({ path, range, ramBytes }: { path: string; range?: string; ramBytes?: number }) {
  const r: Range = range && range in RANGES ? (range as Range) : "1h";
  const [hours, resolution] = RANGES[r];
  // `to` is left to the Server's clock (default now).
  const from = new Date(Date.now() - hours * 3600_000).toISOString();
  const h = await api<HistoryData>(`${path}/history?resolution=${resolution}&from=${from}`);
  // shortcut: 7d/90d fetch every rollup (up to 130k) and thin here; add server-side downsampling if that gets slow.
  const step = Math.ceil(h.samples.length / MAX_POINTS) || 1;
  const s: Resources[] = [];
  for (let i = 0; i < h.samples.length; i += step) s.push(peak(h.samples.slice(i, i + step).map((p) => p.resources)));
  const mounts = [...new Set(s.flatMap((x) => Object.keys(x.disk_used_percent ?? {})))];

  return (
    <section className="card" aria-label="Biểu đồ">
      <div className="row" style={{ justifyContent: "space-between", marginBottom: 10 }}>
        <h3 style={{ margin: 0 }}>Biểu đồ</h3>
        <span className="row muted">
          {h.resolution === "5s" ? "mẫu 5 giây" : "trung bình mỗi phút"} ·
          {(Object.keys(RANGES) as Range[]).map((k) => (
            <Link key={k} href={`?range=${k}`} aria-current={k === r ? "page" : undefined}>
              {k === r ? <b>{k}</b> : k}
            </Link>
          ))}
        </span>
      </div>
      {s.length === 0 && <div className="muted">Chưa có dữ liệu trong khoảng này.</div>}
      <div className="grid" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))" }}>
        <Chart label="CPU" max={100} fmt={pct} series={[{ name: "CPU", values: s.map((x) => x.cpu_percent) }]} />
        <Chart label="RAM" max={ramBytes} fmt={gb} series={[{ name: "RAM", values: s.map((x) => x.ram_used_bytes) }]} />
        <Chart
          label="Disk"
          max={100}
          fmt={pct}
          series={mounts.map((m) => ({ name: m, values: s.map((x) => x.disk_used_percent?.[m]) }))}
        />
        <Chart
          label="Mạng"
          fmt={rate}
          series={[
            { name: "↑", values: s.map((x) => x.net_up_bytes_per_sec) },
            { name: "↓", values: s.map((x) => x.net_down_bytes_per_sec) },
          ]}
        />
        <Chart label="GPU" max={100} fmt={pct} series={[{ name: "GPU", values: s.map((x) => x.gpu_percent) }]} />
      </div>
    </section>
  );
}

/** One point for a bucket of samples: each metric's maximum, so short spikes survive thinning. */
function peak(bucket: Resources[]): Resources {
  const max = (a?: number, b?: number) => (a === undefined ? b : b === undefined ? a : Math.max(a, b));
  return bucket.reduce((acc, r) => {
    const out: Record<string, unknown> = { ...acc };
    for (const [k, v] of Object.entries(r)) {
      if (typeof v === "number") out[k] = max(acc[k as keyof Resources] as number | undefined, v);
    }
    const disks = { ...acc.disk_used_percent };
    for (const [m, v] of Object.entries(r.disk_used_percent ?? {})) disks[m] = max(disks[m], v)!;
    return { ...(out as Resources), disk_used_percent: disks };
  });
}

/** Line chart; points are spaced by sample index (gaps in the data are not drawn as gaps). */
function Chart({ label, series, max, fmt }: { label: string; series: Series[]; max?: number; fmt: (v: number) => string }) {
  const W = 280;
  const H = 70;
  const all = series.flatMap((x) => x.values).filter((v) => v !== undefined);
  const top = max ?? Math.max(1, ...all);
  return (
    <figure aria-label={label} style={{ margin: 0 }}>
      <figcaption className="row muted" style={{ justifyContent: "space-between" }}>
        <span>{label}</span>
        <span className="mono">
          {series.map((x, i) => {
            const last = x.values.findLast((v) => v !== undefined);
            return (
              <span key={x.name} style={{ color: COLORS[i % COLORS.length], marginLeft: 8 }}>
                {series.length > 1 && `${x.name} `}
                {last === undefined ? "—" : fmt(last)}
              </span>
            );
          })}
        </span>
      </figcaption>
      <svg role="img" aria-label={label} viewBox={`0 0 ${W} ${H}`} width="100%" height={H}>
        {series.map((x, i) => (
          <polyline
            key={x.name}
            fill="none"
            stroke={COLORS[i % COLORS.length]}
            strokeWidth={2}
            points={x.values
              .map((v, j) => (v === undefined ? "" : `${(j / Math.max(1, x.values.length - 1)) * W},${H - 2 - (v / top) * (H - 4)}`))
              .filter(Boolean)
              .join(" ")}
          />
        ))}
      </svg>
    </figure>
  );
}
