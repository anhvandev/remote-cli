import { headers } from "next/headers";
import { api, type ServerSettings } from "@/lib/api";
import { card, cardHead, title } from "../styles";
import { CopyButton } from "./copy-button";
import { HttpWarning } from "./http-warning";

/** One copyable command or value. */
function CopyRow({ label, value }: { label?: string; value: string }) {
  return (
    <div className="grid gap-1.5">
      {label && <p className="text-xs text-muted-foreground">{label}</p>}
      <div className="flex items-start justify-between gap-3">
        <code className="rounded-md bg-muted px-2 py-1.5 font-mono text-xs break-all">{value}</code>
        <CopyButton value={value} />
      </div>
    </div>
  );
}

/** The Connect guide (ticket 18): everything a new install or Controller needs. */
export default async function ConnectGuidePage() {
  const settings = await api<ServerSettings>("/api/v1/server-settings");
  const h = await headers();
  const proto = (h.get("x-forwarded-proto") ?? "http").split(",")[0].trim();
  const host = (h.get("x-forwarded-host") ?? h.get("host") ?? "localhost").split(",")[0].trim();
  const origin = `${proto}://${host}`;
  const key = settings.server_key;
  const server = settings.server_address.split(",")[0].trim();

  return (
    <>
      <h2 className="mb-5 text-xl font-semibold tracking-tight">Hướng dẫn kết nối</h2>
      <HttpWarning claimedHttps={proto === "https"} />

      <div className="grid items-start gap-5 lg:grid-cols-2">
        <section className={card} aria-label="Server Address">
          <div className={cardHead}>
            <h3 className={title}>Server Address</h3>
          </div>
          <div className="grid gap-3 p-5 text-sm">
            <CopyRow value={settings.server_address} />
            <p className="text-muted-foreground">Tên Server mà Agent và Controller dùng để kết nối.</p>
          </div>
        </section>

        <section className={card} aria-label="Server Key">
          <div className={cardHead}>
            <h3 className={title}>Server Key</h3>
          </div>
          <div className="grid gap-3 p-5 text-sm">
            <CopyRow value={key} />
            <p className="text-muted-foreground">
              Bí mật mọi Agent và Controller cần để kết nối Server. Máy mới chỉ cần Server Address và Server Key này.
            </p>
          </div>
        </section>
      </div>

      <div className="mt-5 grid items-start gap-5">
        <section className={card} aria-label="Cài đặt Agent">
          <div className={cardHead}>
            <h3 className={title}>Cài đặt Agent</h3>
          </div>
          <div className="grid gap-3 p-5 text-sm">
            <p className="text-muted-foreground">
              Chạy trên máy cần quản lý: script tải <code className="font-mono text-xs">rcli</code> và cài dịch vụ nền,
              Device tự đăng ký vào fleet.
            </p>
            <CopyRow label="Linux / macOS (curl)" value={`curl -fsSL ${origin}/install.sh | sudo sh -s -- --key ${key}`} />
            <CopyRow label="Windows (PowerShell)" value={`iwr ${origin}/install.ps1 -OutFile install.ps1; .\\install.ps1 -Key ${key}`} />
          </div>
        </section>

        <section className={card} aria-label="Tải xuống">
          <div className={cardHead}>
            <h3 className={title}>Tải xuống</h3>
          </div>
          <div className="grid gap-3 p-5 text-sm">
            {/* shortcut: the exe/dmg/deb/rpm packages land with tickets 16/22/24; until then every row serves the
                raw rolled-out build from /download/<os>/<arch> (ticket 17's endpoint). */}
            <ul className="grid gap-1.5">
              <li>
                <a className="font-medium text-foreground underline underline-offset-4" href="/download/windows/x86_64">
                  rcli.exe
                </a>{" "}
                <span className="text-muted-foreground">— Windows</span>
              </li>
              <li>
                <a className="font-medium text-foreground underline underline-offset-4" href="/download/macos/aarch64">
                  rcli.dmg
                </a>{" "}
                <span className="text-muted-foreground">— macOS (Apple silicon)</span>
              </li>
              <li>
                <a className="font-medium text-foreground underline underline-offset-4" href="/download/linux/x86_64">
                  rcli.deb
                </a>{" "}
                <span className="text-muted-foreground">— Debian/Ubuntu</span>
              </li>
              <li>
                <a className="font-medium text-foreground underline underline-offset-4" href="/download/linux/x86_64">
                  rcli.rpm
                </a>{" "}
                <span className="text-muted-foreground">— Fedora/RHEL</span>
              </li>
            </ul>
            <p className="text-muted-foreground">
              Bản dựng chưa ký, nên Windows và macOS hiện cảnh báo khi mở — xem mục cuối trang.
            </p>
          </div>
        </section>

        <section className={card} aria-label="Cài đặt không giao diện">
          <div className={cardHead}>
            <h3 className={title}>Cài đặt không giao diện</h3>
          </div>
          <div className="grid gap-3 p-5 text-sm">
            <p className="text-muted-foreground">
              Trên máy chủ không có màn hình: tải <code className="font-mono text-xs">rcli</code> cho đúng hệ điều hành ở
              trên, rồi chạy:
            </p>
            <CopyRow value={`sudo rcli agent install --server ${server} --key ${key}`} />
          </div>
        </section>

        <section className={card} aria-label="Thiết lập Controller">
          <div className={cardHead}>
            <h3 className={title}>Thiết lập Controller</h3>
          </div>
          <div className="grid gap-3 p-5 text-sm">
            <p className="text-muted-foreground">
              Trên máy điều khiển, chạy <code className="font-mono text-xs">rcli setup</code> một lần với URL của
              Controller API (ví dụ https://server.example.com:8080) và Server Key ở trên:
            </p>
            <CopyRow value={`rcli setup --server <Controller API URL> --key ${key}`} />
            <p className="text-muted-foreground">
              Rồi kết nối Device bằng Device ID và Device Password (hiện ở máy đó qua{" "}
              <code className="font-mono text-xs">rcli agent info</code>):
            </p>
            <CopyRow value="rcli connect <Device ID>" />
          </div>
        </section>

        <section className={card} aria-label="MCP cho Claude Code">
          <div className={cardHead}>
            <h3 className={title}>MCP cho Claude Code</h3>
          </div>
          <div className="grid gap-3 p-5 text-sm">
            <p className="text-muted-foreground">
              Sau khi đã <code className="font-mono text-xs">rcli setup</code> và{" "}
              <code className="font-mono text-xs">rcli connect</code>, thêm <code className="font-mono text-xs">rcli</code>{" "}
              làm MCP server (dùng cùng cấu hình và Device Password đã nhớ):
            </p>
            <CopyRow value="claude mcp add rcli -- rcli mcp" />
          </div>
        </section>

        <section className={card} aria-label="SmartScreen và Gatekeeper">
          <div className={cardHead}>
            <h3 className={title}>SmartScreen và Gatekeeper</h3>
          </div>
          <div className="grid gap-3 p-5 text-sm">
            <p className="text-muted-foreground">
              Bản dựng chưa code-sign, nên hai hệ điều hành hiện cảnh báo khi mở lần đầu:
            </p>
            <p>
              <strong className="font-medium text-foreground">Windows (SmartScreen):</strong>{" "}
              <span className="text-muted-foreground">
                khi mở <code className="font-mono text-xs">rcli.exe</code>, Windows hiện &quot;Windows protected your
                PC&quot; — bấm <em>More info</em> rồi <em>Run anyway</em>.
              </span>
            </p>
            <p>
              <strong className="font-medium text-foreground">macOS (Gatekeeper):</strong>{" "}
              <span className="text-muted-foreground">
                bấm chuột phải vào app rồi chọn <em>Mở</em> (Open). Nếu vẫn bị chặn, gỡ cờ cách ly:{" "}
                <code className="font-mono text-xs">xattr -d com.apple.quarantine {"<đường dẫn>"}</code>.
              </span>
            </p>
          </div>
        </section>
      </div>
    </>
  );
}
