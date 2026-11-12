import { api, type ServerSettings } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Form } from "../form";
import { card, cardHead, title } from "../../styles";
import * as act from "./actions";

export default async function SettingsPage() {
  const settings = await api<ServerSettings>("/api/v1/server-settings");

  return (
    <div className="grid items-start gap-5 lg:grid-cols-2">
      <section className={card} aria-label="Server Key">
        <div className={cardHead}>
          <h3 className={title}>Server Key</h3>
        </div>
        <div className="grid gap-3 p-5 text-sm">
          <code className="rounded-md bg-muted px-2 py-1.5 font-mono text-xs break-all">{settings.server_key}</code>
          <p className="text-muted-foreground">
            Agent và Controller dùng key này để kết nối. Thay key chỉ đổi phần secret: Device đã đăng ký vẫn Online,
            nhưng bản cài mới và Controller cần key mới.
          </p>
          <Form action={act.rotateServerKey} confirm="Thay Server Key? Bản cài mới và Controller cần key mới.">
            <Button size="sm" variant="secondary">
              Thay Server Key
            </Button>
          </Form>
        </div>
      </section>

      <section className={card} aria-label="Server Address">
        <div className={cardHead}>
          <h3 className={title}>Server Address</h3>
        </div>
        <div className="grid gap-3 p-5 text-sm">
          <Form action={act.setServerAddress} className="grid gap-2">
            <Input name="address" aria-label="Server Address" defaultValue={settings.server_address} required className="h-8" />
            <div>
              <Button size="sm" variant="secondary">
                Lưu Server Address
              </Button>
            </div>
          </Form>
          <p className="text-muted-foreground">
            Tên Agent và Controller dùng để tới Server. Tên này nằm trong chứng chỉ TLS; đổi nó sẽ cấp lại chứng chỉ
            và Agent kết nối lại bằng tên mới.
          </p>
        </div>
      </section>

      <section className={card} aria-label="API Address">
        <div className={cardHead}>
          <h3 className={title}>API Address</h3>
        </div>
        <div className="grid gap-3 p-5 text-sm">
          <Form action={act.setApiAddress} className="grid gap-2">
            <Input
              name="address"
              aria-label="API Address"
              defaultValue={settings.api_address}
              placeholder="https://rcli.example.com"
              required
              className="h-8"
            />
            <div>
              <Button size="sm" variant="secondary">
                Lưu API Address
              </Button>
            </div>
          </Form>
          <p className="text-muted-foreground">
            Địa chỉ https:// công khai của Dashboard, nơi Controller gọi tới Server. Server tự ghi lại lần đầu một User mở
            Dashboard qua https:// (không tính http://, localhost hay IP nội bộ). Sửa ở đây nếu nó sai hoặc Dashboard
            đổi địa chỉ: mọi Agent nhận địa chỉ mới ở lần kết nối sau.
          </p>
        </div>
      </section>

      <section className={card} aria-label="Minimum Version">
        <div className={cardHead}>
          <h3 className={title}>Minimum Version</h3>
        </div>
        <div className="grid gap-3 p-5 text-sm">
          <Form action={act.setMinimumVersion} className="grid gap-2">
            <Input
              name="version"
              aria-label="Minimum Version"
              defaultValue={settings.minimum_version}
              placeholder="vd. 0.3.0 (để trống = không có)"
              className="h-8"
            />
            <div>
              <Button size="sm" variant="secondary">
                Lưu Minimum Version
              </Button>
            </div>
          </Form>
          <p className="text-muted-foreground">
            Device có Agent cũ hơn bản này hiện nhãn "cần cập nhật" và tự cập nhật lên Latest Build (thử lại mỗi giờ nếu
            lỗi). Không Device nào bị chặn. Để trống để bỏ.
          </p>
        </div>
      </section>
    </div>
  );
}
