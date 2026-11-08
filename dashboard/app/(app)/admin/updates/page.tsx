import { api, type Device } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import * as act from "../actions";
import { Form } from "../form";
import { card, cardHead, rollouts as groupRollouts, title } from "../shared";

export default async function UpdatesPage() {
  const devices = await api<Device[]>("/api/v1/devices");
  const rollouts = groupRollouts(devices);

  return (
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
  );
}
