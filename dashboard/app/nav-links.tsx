"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";

// On the ink plate the current stop is the one paper cell.
const item =
  "px-2.5 py-2 text-sm font-medium whitespace-nowrap transition-colors duration-(--dur-micro) focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-paper";
const tone = (active: boolean) => `${item} ${active ? "bg-paper text-ink" : "text-paper-2 hover:bg-ink-2 hover:text-paper"}`;

const adminPages = [
  { href: "/admin/users", name: "User" },
  { href: "/admin/updates", name: "Cập nhật Agent" },
  { href: "/admin/settings", name: "Server Settings" },
];

export function NavLinks() {
  const path = usePathname();
  const onFleet = path === "/" || path.startsWith("/devices");
  const onAdmin = path === "/admin";
  return (
    <div className="flex flex-1 flex-wrap gap-1 md:flex-col">
      <Link href="/" aria-current={onFleet ? "page" : undefined} className={tone(onFleet)}>
        Device
      </Link>
      <Link
        href="/connect-guide"
        aria-current={path.startsWith("/connect-guide") ? "page" : undefined}
        className={tone(path.startsWith("/connect-guide"))}
      >
        Hướng dẫn kết nối
      </Link>
      <Link href="/admin" aria-current={onAdmin ? "page" : undefined} className={tone(onAdmin)}>
        Quản lý
      </Link>
      <div role="group" aria-label="Quản lý" className="flex flex-wrap gap-1 md:ml-3 md:flex-col md:gap-0.5 md:border-l-2 md:border-ink-rule md:pl-2">
        {adminPages.map((p) => (
          <Link
            key={p.href}
            href={p.href}
            aria-current={path.startsWith(p.href) ? "page" : undefined}
            className={tone(path.startsWith(p.href))}
          >
            {p.name}
          </Link>
        ))}
      </div>
    </div>
  );
}
