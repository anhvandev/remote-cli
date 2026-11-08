import { UserPlus } from "lucide-react";
import { api, type ApiToken, type Device, type UnlockPassword, type User } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Nav } from "../nav";
import { Status } from "../status";
import * as act from "./actions";
import { Form } from "./form";

const day = (t: string) => new Date(t).toLocaleString("vi-VN");

const card = "rounded-xl border border-border bg-card";
const cardHead = "flex flex-wrap items-center justify-between gap-3 border-b border-border px-5 py-3.5";
const title = "text-sm font-medium text-muted-foreground";
const nativeSelect =
  "h-8 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50";

export default async function AdminPage() {
  const me = await api<User>("/api/v1/me");
  if (me.role !== "admin") {
    return (
      <>
        <Nav />
        <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
          <p className="text-sm text-muted-foreground">Chỉ admin mới xem được trang Quản trị.</p>
        </main>
      </>
    );
  }
  const [users, tokens, devices] = await Promise.all([
    api<User[]>("/api/v1/users"),
    api<ApiToken[]>("/api/v1/tokens"),
    api<Device[]>("/api/v1/devices"),
  ]);
  const unlock = await Promise.all(
    devices.map((d) => api<UnlockPassword>(`/api/v1/devices/${d.id}/unlock-password`)),
  );
  // Rollout progress per version: Devices it was rolled out to, and how many run it / failed.
  const rollouts = new Map<string, { total: number; updated: number; failed: number }>();
  for (const d of devices) {
    if (!d.update) continue;
    const r = rollouts.get(d.update.version) ?? { total: 0, updated: 0, failed: 0 };
    r.total++;
    if (d.update.status === "updated") r.updated++;
    if (d.update.status === "failed") r.failed++;
    rollouts.set(d.update.version, r);
  }

  return (
    <>
      <Nav />
      <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
        <div className="mx-auto max-w-6xl">
          <h2 className="mb-5 text-xl font-semibold tracking-tight">Quản trị</h2>
          <div className="grid items-start gap-5 xl:grid-cols-2">
            <section className={`${card} xl:col-span-2`} aria-label="User & API token">
              <div className={cardHead}>
                <h3 className={title}>User &amp; API token</h3>
                <Form action={act.createUser}>
                  <Input name="name" aria-label="Tên user mới" placeholder="tên user" required className="h-8 w-40" />
                  <select name="role" aria-label="Vai trò" defaultValue="operator" className={nativeSelect}>
                    <option value="operator">operator</option>
                    <option value="admin">admin</option>
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
                          <Badge variant="secondary">{u.role}</Badge>
                        </TableCell>
                        <TableCell className="whitespace-normal">
                          {mine.length} token
                          {mine.map((t) => (
                            <Form key={t.id} action={act.revokeToken} confirm="Thu hồi token này?">
                              <input type="hidden" name="id" value={t.id} />
                              <span className="font-mono text-xs text-muted-foreground">
                                {t.id.slice(0, 8)} · {day(t.created_at)}
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

            <section className={card} aria-label="Enrollment Code">
              <div className={cardHead}>
                <h3 className={title}>Enrollment Code</h3>
                <Form action={act.createEnrollmentCode}>
                  <Button size="sm">+ Tạo mã</Button>
                </Form>
              </div>
              <div className="px-5 py-4">
                <p className="text-sm text-muted-foreground">Mã chỉ hiện một lần; Server chỉ lưu bản băm.</p>
              </div>
            </section>

            <section className={card} aria-label="Mật khẩu unlock">
              <div className={cardHead}>
                <h3 className={title}>Mật khẩu unlock</h3>
              </div>
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead>Device</TableHead>
                    <TableHead>Mật khẩu</TableHead>
                    <TableHead />
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {devices.map((d, i) => (
                    <TableRow key={d.id}>
                      <TableCell>{d.name}</TableCell>
                      <TableCell className="text-muted-foreground">
                        {unlock[i].set ? `đã lưu · không hiển thị (${day(unlock[i].updated_at!)})` : "chưa có"}
                      </TableCell>
                      <TableCell className="whitespace-normal">
                        <div className="flex flex-wrap items-center gap-1.5">
                          <Form action={act.setUnlockPassword}>
                            <input type="hidden" name="device" value={d.id} />
                            <Input
                              type="password"
                              name="password"
                              aria-label={`Mật khẩu unlock cho ${d.name}`}
                              autoComplete="off"
                              required
                              className="h-8 w-36"
                            />
                            <Button size="xs" variant="outline">
                              Lưu
                            </Button>
                          </Form>
                          {unlock[i].set && (
                            <Form action={act.clearUnlockPassword}>
                              <input type="hidden" name="device" value={d.id} />
                              <Button size="xs" variant="ghost" className="text-destructive">
                                Xoá
                              </Button>
                            </Form>
                          )}
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </section>

            <section className={card} aria-label="Cập nhật Agent">
              <div className={cardHead}>
                <h3 className={title}>Cập nhật Agent</h3>
              </div>
              <div className="grid gap-5 p-5">
                <Form action={act.uploadBuild} className="grid gap-2">
                  <Input type="file" name="build" aria-label="File build" required className="h-9 py-1.5" />
                  <div className="flex flex-wrap gap-2">
                    <Input name="version" aria-label="Phiên bản" placeholder="phiên bản" required className="h-8 w-32" />
                    <Input name="target" aria-label="Target" placeholder="vd. macos-aarch64" required className="h-8 w-44" />
                  </div>
                  <Input
                    name="signature"
                    aria-label="Chữ ký (hex)"
                    placeholder="chữ ký Ed25519 (hex)"
                    className="h-8 font-mono"
                    required
                  />
                  <div>
                    <Button size="sm" variant="secondary">
                      Tải lên
                    </Button>
                  </div>
                </Form>
                <div className="border-t border-border pt-4">
                  <Form action={act.rollout} label="Rollout" className="grid gap-3">
                    <Input name="version" aria-label="Phiên bản" placeholder="phiên bản cần rollout" required className="h-8" />
                    <div className="flex flex-wrap gap-x-4 gap-y-2">
                      {devices.map((d) => (
                        <div key={d.id} className="flex items-center gap-1.5">
                          <Checkbox id={`rollout-${d.id}`} name="devices" value={d.id} aria-label={d.name} />
                          <Label htmlFor={`rollout-${d.id}`} className="text-xs font-normal text-muted-foreground">
                            {d.name}
                          </Label>
                        </div>
                      ))}
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button size="sm" variant="secondary" name="scope" value="all">
                        Cập nhật tất cả
                      </Button>
                      <Button size="sm" variant="outline" name="scope" value="selected">
                        Cập nhật Device đã chọn
                      </Button>
                    </div>
                  </Form>
                </div>
                <Table>
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <TableHead>Phiên bản</TableHead>
                      <TableHead>Tiến độ</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {[...rollouts].map(([version, r]) => (
                      <TableRow key={version}>
                        <TableCell>{version}</TableCell>
                        <TableCell>
                          <span className="mr-2 inline-block h-1.5 w-16 overflow-hidden rounded-full bg-border align-middle">
                            <span
                              className="block h-full rounded-full bg-primary"
                              style={{ width: `${(100 * r.updated) / r.total}%` }}
                            />
                          </span>
                          {r.updated}/{r.total}
                          {r.failed > 0 && <span className="text-destructive"> · {r.failed} lỗi</span>}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            </section>

            <section className={`${card} xl:col-span-2`} aria-label="Device">
              <div className={cardHead}>
                <h3 className={title}>Device</h3>
                <span className="text-xs text-muted-foreground">Đổi tên, gộp lịch sử, thu hồi hoặc xoá</span>
              </div>
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead>Device</TableHead>
                    <TableHead>Trạng thái</TableHead>
                    <TableHead>Agent</TableHead>
                    <TableHead />
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {devices.map((d) => (
                    <TableRow key={d.id}>
                      <TableCell>
                        {d.name} <span className="font-mono text-xs text-muted-foreground">{d.id.slice(0, 8)}</span>
                      </TableCell>
                      <TableCell>
                        <Status status={d.status} />{" "}
                        {d.revoked_at && (
                          <Badge variant="outline" className="border-warn/40 text-warn">
                            đã thu hồi
                          </Badge>
                        )}
                      </TableCell>
                      <TableCell className="text-muted-foreground">
                        {d.agent_version ?? "—"}
                        {d.update && (
                          <>
                            {" → "}
                            {d.update.version} · {d.update.status}
                            {d.update.error && <span className="text-destructive"> ({d.update.error})</span>}
                          </>
                        )}
                      </TableCell>
                      <TableCell className="whitespace-normal">
                        <div className="flex flex-wrap items-center gap-1.5">
                          <Form action={act.renameDevice}>
                            <input type="hidden" name="device" value={d.id} />
                            <Input name="name" aria-label={`Tên mới cho ${d.name}`} placeholder="tên mới" required className="h-8 w-28" />
                            <Button size="xs" variant="outline">
                              Đổi tên
                            </Button>
                          </Form>
                          <Form action={act.mergeDevice} confirm={`Gộp lịch sử vào ${d.name} rồi xoá Device cũ?`}>
                            <input type="hidden" name="device" value={d.id} />
                            <select
                              name="from"
                              aria-label={`Gộp lịch sử từ Device vào ${d.name}`}
                              required
                              defaultValue=""
                              className={nativeSelect}
                            >
                              <option value="" disabled>
                                Gộp lịch sử từ…
                              </option>
                              {devices
                                .filter((o) => o.id !== d.id)
                                .map((o) => (
                                  <option key={o.id} value={o.id}>
                                    {o.name}
                                  </option>
                                ))}
                            </select>
                            <Button size="xs" variant="outline">
                              Gộp
                            </Button>
                          </Form>
                          {!d.revoked_at && (
                            <Form action={act.revokeDevice} confirm={`Thu hồi ${d.name}? Device không thể kết nối lại.`}>
                              <input type="hidden" name="device" value={d.id} />
                              <Button size="xs" variant="ghost" className="text-destructive">
                                Thu hồi
                              </Button>
                            </Form>
                          )}
                          <Form action={act.deleteDevice} confirm={`Xoá ${d.name} cùng lịch sử của nó?`}>
                            <input type="hidden" name="device" value={d.id} />
                            <Button size="xs" variant="ghost" className="text-destructive">
                              Xoá
                            </Button>
                          </Form>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </section>
          </div>
        </div>
      </main>
    </>
  );
}
