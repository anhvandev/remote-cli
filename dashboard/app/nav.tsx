import Link from "next/link";
import { api, type User } from "@/lib/api";
import { signOut } from "./actions";

export async function Nav() {
  const me = await api<User>("/api/v1/me");
  return (
    <nav>
      <b>⌘ Remote CLI</b>
      <Link href="/">Danh sách Device</Link>
      <span className="who">
        {me.name} · {me.role}
        <form action={signOut}>
          <button className="btn">Đăng xuất</button>
        </form>
      </span>
    </nav>
  );
}
