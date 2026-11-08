import { api, type User } from "@/lib/api";

/** Admin-only guard for every page under Quản trị. */
export default async function AdminLayout({ children }: { children: React.ReactNode }) {
  const me = await api<User>("/api/v1/me");
  if (me.role !== "admin") {
    return (
      <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
        <p className="text-sm text-muted-foreground">Chỉ admin mới xem được trang Quản trị.</p>
      </main>
    );
  }
  return (
    <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
      <div className="mx-auto max-w-6xl">
        <h2 className="mb-5 text-xl font-semibold tracking-tight">Quản trị</h2>
        {children}
      </div>
    </main>
  );
}
