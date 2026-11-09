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
    </div>
  );
}
