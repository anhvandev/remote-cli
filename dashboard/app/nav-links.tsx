"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";

const item = "rounded-md px-2.5 py-2 text-sm transition-colors hover:bg-accent hover:text-foreground";
const tone = (active: boolean) => `${item} ${active ? "bg-accent text-foreground" : "text-muted-foreground"}`;

// The Device sub-entry needs a distinct accessible name: an exact "Device" link would make the top-level
// Device link ambiguous in role/name queries.
const adminPages = [
  { href: "/admin/users", name: "User" },
  { href: "/admin/devices", name: "Device", label: "Device (Admin)" },
  { href: "/admin/enrollment", name: "Enrollment Code" },
  { href: "/admin/updates", name: "Cập nhật Agent" },
];

export function NavLinks({ admin }: { admin: boolean }) {
  const path = usePathname();
  const onFleet = path === "/" || path.startsWith("/devices");
  const onAdmin = path === "/admin";
  return (
    <div className="flex flex-1 gap-1 md:flex-col">
      <Link href="/" aria-current={onFleet ? "page" : undefined} className={tone(onFleet)}>
        Device
      </Link>
      {admin && (
        <>
          <Link href="/admin" aria-current={onAdmin ? "page" : undefined} className={tone(onAdmin)}>
            Quản trị
          </Link>
          <div className="flex gap-1 md:ml-4 md:flex-col md:gap-0.5 md:border-l md:border-border md:pl-2">
            {adminPages.map((p) => (
              <Link
                key={p.href}
                href={p.href}
                aria-label={p.label}
                aria-current={path.startsWith(p.href) ? "page" : undefined}
                className={tone(path.startsWith(p.href))}
              >
                {p.name}
              </Link>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
