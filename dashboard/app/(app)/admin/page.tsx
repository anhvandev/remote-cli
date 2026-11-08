import Link from "next/link";
import { ChevronRight } from "lucide-react";

// Ticket 07 replaces this with the four-card overview.
const pages = [
  { href: "/admin/users", name: "User" },
  { href: "/admin/devices", name: "Device" },
  { href: "/admin/enrollment", name: "Enrollment Code" },
  { href: "/admin/updates", name: "Cập nhật Agent" },
];

export default function AdminPage() {
  return (
    <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
      <div className="mx-auto max-w-6xl">
        <h2 className="mb-5 text-xl font-semibold tracking-tight">Quản trị</h2>
        <ul className="grid gap-3 sm:grid-cols-2">
          {pages.map((p) => (
            <li key={p.href}>
              <Link
                href={p.href}
                className="flex items-center justify-between rounded-xl border border-border bg-card px-5 py-4 text-sm font-medium transition-colors hover:bg-accent"
              >
                {p.name}
                <ChevronRight className="size-4 text-muted-foreground" />
              </Link>
            </li>
          ))}
        </ul>
      </div>
    </main>
  );
}
