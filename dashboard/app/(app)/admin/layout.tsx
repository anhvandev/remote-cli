import { api, type User } from "@/lib/api";

/** Admin-only guard for every page under Quản trị. */
export default async function AdminLayout({ children }: { children: React.ReactNode }) {
  const me = await api<User>("/api/v1/me");
  if (me.role !== "admin") return <p className="text-sm text-muted-foreground">Chỉ Admin mới xem được trang Quản trị.</p>;
  return (
    <>
      <h2 className="mb-5 text-xl font-semibold tracking-tight">Quản trị</h2>
      {children}
    </>
  );
}
