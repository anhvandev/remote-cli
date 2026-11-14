import Link from "next/link";
import { api, type ServerSettings, type User } from "@/lib/api";

// The three admin areas as rows of one board: the area on the left, what it holds now on the right.
const row = "grid gap-x-6 gap-y-1 border-b border-rule py-4 last:border-b-0 md:grid-cols-[16rem_minmax(0,1fr)]";
const head = "text-lg font-extrabold tracking-tight";
const link = "underline-offset-4 hover:text-primary hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring";

export default async function AdminPage() {
  const [users, settings] = await Promise.all([
    api<User[]>("/api/v1/users"),
    api<ServerSettings>("/api/v1/server-settings"),
  ]);
  const disabled = users.filter((u) => u.disabled_at).length;

  return (
    <div className="border-b-2 border-ink">
      <section className={row} aria-label="User">
        <h3 className={head}>
          <Link href="/admin/users" className={link}>
            User
          </Link>
        </h3>
        <div className="grid gap-1 text-sm">
          <p>
            <b className="font-extrabold">{users.length - disabled}</b> User đang hoạt động
          </p>
          <p className="text-muted-foreground">
            <b className="font-extrabold">{disabled}</b> User đã vô hiệu
          </p>
        </div>
      </section>

      <section className={row} aria-label="Cập nhật Agent">
        <h3 className={head}>
          <Link href="/admin/updates" className={link}>
            Cập nhật Agent
          </Link>
        </h3>
        <p className="text-sm text-muted-foreground">Tải lên Agent Build; bản cài mới nhận Latest Build.</p>
      </section>

      <section className={row} aria-label="Server Settings">
        <h3 className={head}>
          <Link href="/admin/settings" className={link}>
            Server Settings
          </Link>
        </h3>
        <div className="grid gap-1 text-sm">
          <p>Server Address: {settings.server_address}</p>
          <p className="text-muted-foreground">Server Key và Server Address dùng cho bản cài mới.</p>
        </div>
      </section>
    </div>
  );
}
