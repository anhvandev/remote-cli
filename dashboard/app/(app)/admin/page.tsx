import Link from "next/link";
import { api, type Device, type DeviceStatus, type ServerSettings, type User } from "@/lib/api";
import { card, cardHead, title } from "../styles";

export default async function AdminPage() {
  const [users, devices, settings] = await Promise.all([
    api<User[]>("/api/v1/users"),
    api<Device[]>("/api/v1/devices"),
    api<ServerSettings>("/api/v1/server-settings"),
  ]);
  const count = (role: User["role"]) => users.filter((u) => u.role === role).length;
  const status = (s: DeviceStatus) => devices.filter((d) => d.status === s).length;

  return (
    <div className="grid items-start gap-5 lg:grid-cols-2">
      <section className={card} aria-label="User">
        <div className={cardHead}>
          <h3 className={title}>
            <Link href="/admin/users" className="hover:text-foreground">
              User
            </Link>
          </h3>
        </div>
        <div className="grid gap-1.5 px-5 text-sm">
          <p>
            {count("admin")} Admin · {count("operator")} Operator
          </p>
          <p>{users.filter((u) => u.disabled_at).length} User đã vô hiệu</p>
        </div>
      </section>

      <section className={card} aria-label="Device">
        <div className={cardHead}>
          <h3 className={title}>
            <Link href="/admin/devices" className="hover:text-foreground">
              Device
            </Link>
          </h3>
        </div>
        <div className="grid gap-1.5 px-5 text-sm">
          <p>
            {status("Online")} Online · {status("Offline")} Offline · {status("Waking")} Waking
          </p>
          <p>{devices.filter((d) => d.revoked_at).length} Device đã thu hồi</p>
        </div>
      </section>

      <section className={card} aria-label="Cập nhật Agent">
        <div className={cardHead}>
          <h3 className={title}>
            <Link href="/admin/updates" className="hover:text-foreground">
              Cập nhật Agent
            </Link>
          </h3>
        </div>
        <div className="grid gap-1.5 px-5 text-sm">
          <p className="text-muted-foreground">Tải lên Agent Build; bản cài mới nhận Latest Build.</p>
        </div>
      </section>

      <section className={card} aria-label="Server Settings">
        <div className={cardHead}>
          <h3 className={title}>
            <Link href="/admin/settings" className="hover:text-foreground">
              Server Settings
            </Link>
          </h3>
        </div>
        <div className="grid gap-1.5 px-5 text-sm">
          <p>Server Address: {settings.server_address}</p>
          <p className="text-muted-foreground">Server Key và Server Address dùng cho bản cài mới.</p>
        </div>
      </section>
    </div>
  );
}
