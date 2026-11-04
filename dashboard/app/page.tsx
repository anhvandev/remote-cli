import Link from "next/link";
import { api, type Device } from "@/lib/api";
import { Nav } from "./nav";
import { Refresh } from "./refresh";
import { Status } from "./status";

export default async function FleetPage() {
  const devices = await api<Device[]>("/api/v1/devices");
  const count = (s: string) => devices.filter((d) => d.status === s).length;
  return (
    <>
      <Nav />
      <Refresh />
      <main>
        <h2>
          Device{" "}
          <span className="muted">
            · {count("Online")} Online · {count("Offline")} Offline · {count("Waking")} Waking
          </span>
        </h2>
        <div className="card" style={{ padding: 0 }}>
          <table>
            <thead>
              <tr>
                <th>Device</th>
                <th>Trạng thái</th>
                <th>Lần cuối thấy</th>
              </tr>
            </thead>
            <tbody>
              {devices.map((d) => (
                <tr key={d.id}>
                  <td>
                    <Link href={`/devices/${d.id}`}>
                      <b>{d.name}</b>
                    </Link>
                  </td>
                  <td>
                    <Status status={d.status} wakeError={d.wake_error} />
                  </td>
                  <td className="muted">{d.last_seen_at ? new Date(d.last_seen_at).toLocaleString("vi-VN") : "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {devices.length === 0 && <p className="muted" style={{ padding: 14 }}>Chưa có Device nào.</p>}
        </div>
      </main>
    </>
  );
}
