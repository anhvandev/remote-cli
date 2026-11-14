import Link from "next/link";
import { api, type User } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { signOut } from "./actions";
import { NavLinks } from "./nav-links";

export async function Nav() {
  const me = await api<User>("/api/v1/me");
  return (
    <nav className="flex w-full shrink-0 flex-col bg-ink px-3 py-3 text-paper md:sticky md:top-0 md:h-screen md:w-60 md:px-3 md:py-5">
      <Link href="/" aria-label="Remote CLI, về danh sách Device" className="mb-6 px-2 text-lg font-extrabold tracking-tight">
        Remote CLI
      </Link>
      <NavLinks />
      <div className="mt-4 flex items-center justify-between gap-2 border-t-2 border-ink-rule pt-4 md:flex-col md:items-stretch">
        <div className="min-w-0 truncate px-2 text-sm font-medium">{me.name}</div>
        <form action={signOut}>
          <Button variant="ghost" size="sm" className="w-full justify-start px-2 font-medium text-paper-2 hover:bg-ink-2 hover:text-paper focus-visible:outline-paper md:mt-1">
            Đăng xuất
          </Button>
        </form>
      </div>
    </nav>
  );
}
