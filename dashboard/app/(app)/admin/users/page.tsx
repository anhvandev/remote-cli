import { api, type ApiToken, type User } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import * as act from "../actions";
import { Form } from "../form";
import { card, cardHead, nativeSelect, title } from "../../styles";

export default async function UsersPage() {
  const [users, tokens] = await Promise.all([api<User[]>("/api/v1/users"), api<ApiToken[]>("/api/v1/tokens")]);
  return (
    <section className={card} aria-label="User & API token">
      <div className={cardHead}>
        <h3 className={title}>User &amp; API token</h3>
        <Form action={act.createUser}>
          <Input name="name" aria-label="Tên user mới" placeholder="tên user" required className="h-8 w-40" />
          <select name="role" aria-label="Vai trò" defaultValue="operator" className={nativeSelect}>
            <option value="operator">Operator</option>
            <option value="admin">Admin</option>
          </select>
          <Button size="sm" variant="secondary">
            + User
          </Button>
        </Form>
      </div>
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead>User</TableHead>
            <TableHead>Vai trò</TableHead>
            <TableHead>Token</TableHead>
            <TableHead />
          </TableRow>
        </TableHeader>
        <TableBody>
          {users.map((u) => {
            const mine = tokens.filter((t) => t.user === u.name);
            return (
              <TableRow key={u.id}>
                <TableCell className="whitespace-normal">
                  {u.name}{" "}
                  {u.disabled_at && (
                    <Badge variant="outline" className="border-warn/40 text-warn">
                      đã vô hiệu
                    </Badge>
                  )}
                </TableCell>
                <TableCell>
                  <Badge variant="secondary">{u.role === "admin" ? "Admin" : "Operator"}</Badge>
                </TableCell>
                <TableCell className="whitespace-normal">
                  {mine.length} token
                  {mine.map((t) => (
                    <Form key={t.id} action={act.revokeToken} confirm="Thu hồi token này?">
                      <input type="hidden" name="id" value={t.id} />
                      <span className="font-mono text-xs text-muted-foreground">
                        {t.id.slice(0, 8)} · {new Date(t.created_at).toLocaleString("vi-VN")}
                      </span>
                      <Button
                        size="icon-xs"
                        variant="ghost"
                        className="text-destructive"
                        aria-label={`Thu hồi token ${t.id.slice(0, 8)}`}
                      >
                        ×
                      </Button>
                    </Form>
                  ))}
                </TableCell>
                <TableCell className="whitespace-normal">
                  {!u.disabled_at && (
                    <div className="flex items-center justify-end gap-1.5">
                      <Form action={act.createToken}>
                        <input type="hidden" name="user" value={u.name} />
                        <Button size="xs" variant="secondary">
                          Tạo token
                        </Button>
                      </Form>
                      <Form action={act.setPassword}>
                        <input type="hidden" name="user" value={u.name} />
                        <Input
                          type="password"
                          name="password"
                          aria-label={`Mật khẩu Dashboard cho ${u.name}`}
                          placeholder="mật khẩu (≥ 8)"
                          autoComplete="new-password"
                          required
                          className="h-8 w-36"
                        />
                        <Button size="xs" variant="outline">
                          Đặt mật khẩu
                        </Button>
                      </Form>
                      <Form action={act.disableUser} confirm={`Vô hiệu ${u.name}? Mọi token của họ ngừng hoạt động.`}>
                        <input type="hidden" name="user" value={u.name} />
                        <Button size="xs" variant="ghost" className="text-destructive">
                          Vô hiệu
                        </Button>
                      </Form>
                    </div>
                  )}
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </section>
  );
}
