"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";

const item = "rounded-md px-2.5 py-2 text-sm transition-colors hover:bg-accent hover:text-foreground";
const tone = (active: boolean) => `${item} ${active ? "bg-accent text-foreground" : "text-muted-foreground"}`;

export function NavLinks({ admin }: { admin: boolean }) {
  const path = usePathname();
  const onFleet = path === "/" || path.startsWith("/devices");
  const onAdmin = path.startsWith("/admin");
  return (
    <div className="flex flex-1 gap-1 md:flex-col">
      <Link href="/" aria-current={onFleet ? "page" : undefined} className={tone(onFleet)}>
        Device
      </Link>
      {admin && (
        <Link href="/admin" aria-current={onAdmin ? "page" : undefined} className={tone(onAdmin)}>
          Quản trị
        </Link>
      )}
    </div>
  );
}
