import Link from "next/link";
import { Server } from "lucide-react";
import { api, type User } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { signOut } from "./actions";
import { NavLinks } from "./nav-links";

export async function Nav() {
  const me = await api<User>("/api/v1/me");
  return (
    <nav className="flex w-full shrink-0 flex-col border-b border-border bg-card/60 px-3 py-3 md:h-screen md:w-60 md:border-r md:border-b-0 md:px-3 md:py-5 md:sticky md:top-0">
      <Link href="/" className="mb-6 flex items-center gap-2 px-2 text-[15px] font-semibold tracking-tight">
        <span className="grid size-7 place-items-center rounded-md bg-primary text-primary-foreground">
          <Server className="size-4" />
        </span>
        Remote CLI
      </Link>
      <NavLinks admin={me.role === "admin"} />
      <div className="mt-4 flex items-center justify-between gap-2 border-t border-border pt-4 md:flex-col md:items-stretch">
        <div className="min-w-0 truncate px-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{me.name}</span> · {me.role === "admin" ? "Admin" : "Operator"}
        </div>
        <form action={signOut}>
          <Button variant="ghost" size="sm" className="w-full justify-start text-muted-foreground md:mt-2">
            Đăng xuất
          </Button>
        </form>
      </div>
    </nav>
  );
}
