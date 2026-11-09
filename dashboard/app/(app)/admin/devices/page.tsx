import { api, type Device } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Status } from "../../../status";
import * as act from "../actions";
import { Form } from "../form";
import { card, cardHead, nativeSelect, title } from "../../styles";
import { unlockPasswords } from "../data";

export default async function DevicesPage() {
  const devices = await api<Device[]>("/api/v1/devices");
  const unlock = await unlockPasswords(devices);
  return (
    <div className="grid items-start gap-5">
      <section className={card} aria-label="Device">
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
                  {d.name} <span className="font-mono text-xs text-muted-foreground">{d.device_id}</span>
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
                  {unlock[i].set ? `đã lưu · không hiển thị (${new Date(unlock[i].updated_at!).toLocaleString("vi-VN")})` : "chưa có"}
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
    </div>
  );
}
