import Link from "next/link";
import { api, type History as HistoryData, type Resources } from "@/lib/api";
import { cn } from "@/lib/utils";

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
  // Axis labels: first and last sample of the window.
  const clock = (t: string) =>
    new Date(t).toLocaleString("vi-VN", r === "1h" ? { hour: "2-digit", minute: "2-digit" } : { day: "2-digit", month: "2-digit", hour: "2-digit" });
  const span = h.samples.length > 1 ? [clock(h.samples[0].at), clock(h.samples[h.samples.length - 1].at)] : ["", ""];

  return (
    <section className="rounded-xl border border-border bg-card" aria-label="Biểu đồ">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-5 py-3.5">
        <h3 className="text-sm font-medium text-muted-foreground">Biểu đồ</h3>
        <span className="flex items-center gap-3 text-xs text-muted-foreground">
          {h.resolution === "5s" ? "mẫu 5 giây" : "trung bình mỗi phút"}
          <span className="flex items-center gap-1 rounded-md border border-border p-0.5">
            {(Object.keys(RANGES) as Range[]).map((k) => (
              <Link
                key={k}
                href={`?range=${k}`}
                aria-current={k === r ? "page" : undefined}
                className={cn(
                  "rounded px-2 py-0.5 transition-colors hover:text-foreground",
                  k === r && "bg-secondary font-medium text-foreground",
                )}
              >
                {k}
              </Link>
            ))}
          </span>
        </span>
      </div>
      <div className="p-5">
        {s.length === 0 && <div className="text-sm text-muted-foreground">Chưa có dữ liệu trong khoảng này.</div>}
        <div className="grid grid-cols-[repeat(auto-fit,minmax(260px,1fr))] gap-5">
          <Chart label="CPU" span={span} max={100} fmt={pct} series={[{ name: "CPU", values: s.map((x) => x.cpu_percent) }]} />
          <Chart label="RAM" span={span} max={ramBytes} fmt={gb} series={[{ name: "RAM", values: s.map((x) => x.ram_used_bytes) }]} />
          <Chart
            label="Disk"
            span={span}
            max={100}
            fmt={pct}
            series={mounts.map((m) => ({ name: m, values: s.map((x) => x.disk_used_percent?.[m]) }))}
          />
          <Chart
            label="Mạng"
            span={span}
            fmt={rate}
            series={[
              { name: "↑", values: s.map((x) => x.net_up_bytes_per_sec) },
              { name: "↓", values: s.map((x) => x.net_down_bytes_per_sec) },
            ]}
          />
          <Chart label="GPU" span={span} max={100} fmt={pct} series={[{ name: "GPU", values: s.map((x) => x.gpu_percent) }]} />
        </div>
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
  }, {} as Resources);
}

/** Line chart; points are spaced by sample index (gaps in the data are not drawn as gaps). */
function Chart({ label, series, max, fmt, span }: { label: string; series: Series[]; max?: number; fmt: (v: number) => string; span: string[] }) {
  const W = 280;
  const H = 70;
  const all = series.flatMap((x) => x.values).filter((v) => v !== undefined);
  const top = max ?? Math.max(1, ...all);
  const y = (v: number) => H - 2 - (v / top) * (H - 4);
  const points = (values: (number | undefined)[]) =>
    values
      .map((v, j) => (v === undefined ? null : { x: (j / Math.max(1, values.length - 1)) * W, y: y(v) }))
      .filter((p): p is { x: number; y: number } => p !== null);
  return (
    <figure aria-label={label} className="m-0">
      <figcaption className="mb-1.5 flex items-center justify-between text-xs text-muted-foreground">
        <span>{label}</span>
        <span className="font-mono">
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
      <svg role="img" aria-label={label} viewBox={`0 0 ${W} ${H}`} width="100%" height={H} className="overflow-visible">
        {[0, 1, 2, 3].map((i) => (
          <line key={i} x1={0} x2={W} y1={(i * H) / 3} y2={(i * H) / 3} stroke="currentColor" strokeOpacity={0.08} strokeWidth={1} />
        ))}
        {series.map((x, i) => {
          const pts = points(x.values);
          if (pts.length === 0) return null;
          const color = COLORS[i % COLORS.length];
          const line = pts.map((p) => `${p.x},${p.y}`).join(" ");
          const area = `0,${H} ${line} ${W},${H}`;
          const last = pts[pts.length - 1];
          return (
            <g key={x.name}>
              <polygon fill={color} fillOpacity={0.12} points={area} />
              <polyline fill="none" stroke={color} strokeWidth={1.75} strokeLinejoin="round" strokeLinecap="round" points={line} />
              <circle cx={last.x} cy={last.y} r={2.5} fill={color} />
            </g>
          );
        })}
      </svg>
      <div className="mt-1 flex justify-between text-[10px] text-muted-foreground">
        <span>{span[0]}</span>
        <span>{span[1]}</span>
      </div>
    </figure>
  );
}
