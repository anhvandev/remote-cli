import { api, type ApiToken, type Device, type UnlockPassword, type User } from "@/lib/api";
import { Nav } from "../nav";
import { Status } from "../status";
import * as act from "./actions";
import { Form } from "./form";

const day = (t: string) => new Date(t).toLocaleString("vi-VN");

export default async function AdminPage() {
  const me = await api<User>("/api/v1/me");
  if (me.role !== "admin") {
    return (
      <>
        <Nav />
        <main>
          <p className="muted">Chỉ admin mới xem được trang Quản trị.</p>
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
      <main>
        <h2>Quản trị</h2>
        <div className="grid" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(420px, 1fr))", alignItems: "start" }}>
          <section className="card" aria-label="User & API token">
            <h3>User &amp; API token</h3>
            <Form action={act.createUser}>
              <input name="name" aria-label="Tên user mới" placeholder="tên user" required />
              <select name="role" aria-label="Vai trò" defaultValue="operator">
                <option value="operator">operator</option>
                <option value="admin">admin</option>
              </select>
              <button className="btn">+ User</button>
            </Form>
            <table>
              <thead>
                <tr>
                  <th>User</th>
                  <th>Vai trò</th>
                  <th>Token</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {users.map((u) => {
                  const mine = tokens.filter((t) => t.user === u.name);
                  return (
                    <tr key={u.id}>
                      <td>
                        {u.name} {u.disabled_at && <span className="tag warn">đã vô hiệu</span>}
                      </td>
                      <td>
                        <span className="tag">{u.role}</span>
                      </td>
                      <td>
                        {mine.length} token
                        {mine.map((t) => (
                          <Form key={t.id} action={act.revokeToken} confirm="Thu hồi token này?">
                            <input type="hidden" name="id" value={t.id} />
                            <span className="muted mono">{t.id.slice(0, 8)} · {day(t.created_at)}</span>
                            <button className="btn danger" aria-label={`Thu hồi token ${t.id.slice(0, 8)}`}>×</button>
                          </Form>
                        ))}
                      </td>
                      <td>
                        {!u.disabled_at && (
                          <div className="grid" style={{ gap: 6 }}>
                            <Form action={act.createToken}>
                              <input type="hidden" name="user" value={u.name} />
                              <button className="btn">Tạo token</button>
                            </Form>
                            <Form action={act.setPassword}>
                              <input type="hidden" name="user" value={u.name} />
                              <input
                                type="password"
                                name="password"
                                aria-label={`Mật khẩu Dashboard cho ${u.name}`}
                                placeholder="mật khẩu Dashboard (≥ 8)"
                                autoComplete="new-password"
                                required
                              />
                              <button className="btn">Đặt mật khẩu</button>
                            </Form>
                            <Form action={act.disableUser} confirm={`Vô hiệu ${u.name}? Mọi token của họ ngừng hoạt động.`}>
                              <input type="hidden" name="user" value={u.name} />
                              <button className="btn danger">Vô hiệu</button>
                            </Form>
                          </div>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </section>

          <section className="card" aria-label="Enrollment Code">
            <h3>Enrollment Code</h3>
            <p className="muted">Mã chỉ hiện một lần; Server chỉ lưu bản băm.</p>
            <Form action={act.createEnrollmentCode}>
              <button className="btn primary">+ Tạo mã</button>
            </Form>
          </section>

          <section className="card" aria-label="Mật khẩu unlock">
            <h3>Mật khẩu unlock</h3>
            <table>
              <thead>
                <tr>
                  <th>Device</th>
                  <th>Mật khẩu</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {devices.map((d, i) => (
                  <tr key={d.id}>
                    <td>{d.name}</td>
                    <td className="muted">
                      {unlock[i].set ? `đã lưu · không hiển thị (${day(unlock[i].updated_at!)})` : "chưa có"}
                    </td>
                    <td>
                      <Form action={act.setUnlockPassword}>
                        <input type="hidden" name="device" value={d.id} />
                        <input
                          type="password"
                          name="password"
                          aria-label={`Mật khẩu unlock cho ${d.name}`}
                          autoComplete="off"
                          required
                        />
                        <button className="btn">Lưu</button>
                      </Form>
                      {unlock[i].set && (
                        <Form action={act.clearUnlockPassword}>
                          <input type="hidden" name="device" value={d.id} />
                          <button className="btn danger">Xoá</button>
                        </Form>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>

          <section className="card" aria-label="Cập nhật Agent">
            <h3>Cập nhật Agent</h3>
            <Form action={act.uploadBuild} className="grid">
              <input type="file" name="build" aria-label="File build" required />
              <div className="row">
                <input name="version" aria-label="Phiên bản" placeholder="phiên bản" required />
                <input name="target" aria-label="Target" placeholder="vd. macos-aarch64" required />
              </div>
              <input name="signature" aria-label="Chữ ký (hex)" placeholder="chữ ký Ed25519 (hex)" className="mono" required />
              <div>
                <button className="btn">Tải lên</button>
              </div>
            </Form>
            <table>
              <thead>
                <tr>
                  <th>Phiên bản</th>
                  <th>Tiến độ</th>
                </tr>
              </thead>
              <tbody>
                {[...rollouts].map(([version, r]) => (
                  <tr key={version}>
                    <td>{version}</td>
                    <td>
                      <span className="bar">
                        <i style={{ width: `${(100 * r.updated) / r.total}%` }} />
                      </span>
                      {r.updated}/{r.total}
                      {r.failed > 0 && <span className="error"> · {r.failed} lỗi</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <Form action={act.rollout} label="Rollout" className="grid">
              <input name="version" aria-label="Phiên bản" placeholder="phiên bản cần rollout" required />
              <div className="row">
                {devices.map((d) => (
                  <label key={d.id}>
                    <input type="checkbox" name="devices" value={d.id} /> {d.name}
                  </label>
                ))}
              </div>
              <div className="row">
                <button className="btn primary" name="scope" value="all">
                  Cập nhật tất cả
                </button>
                <button className="btn" name="scope" value="selected">
                  Cập nhật Device đã chọn
                </button>
              </div>
            </Form>
          </section>

          <section className="card" aria-label="Device" style={{ gridColumn: "1/-1" }}>
            <h3>Device</h3>
            <table>
              <thead>
                <tr>
                  <th>Device</th>
                  <th>Trạng thái</th>
                  <th>Agent</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {devices.map((d) => (
                  <tr key={d.id}>
                    <td>
                      {d.name} <span className="muted mono">{d.id.slice(0, 8)}</span>
                    </td>
                    <td>
                      <Status status={d.status} />{" "}
                      {d.revoked_at && <span className="tag warn">đã thu hồi</span>}
                    </td>
                    <td className="muted">
                      {d.agent_version ?? "—"}
                      {d.update && (
                        <>
                          {" → "}
                          {d.update.version} · {d.update.status}
                          {d.update.error && <span className="error"> ({d.update.error})</span>}
                        </>
                      )}
                    </td>
                    <td>
                      <div className="row" style={{ gap: 6 }}>
                        <Form action={act.renameDevice}>
                          <input type="hidden" name="device" value={d.id} />
                          <input name="name" aria-label={`Tên mới cho ${d.name}`} placeholder="tên mới" required />
                          <button className="btn">Đổi tên</button>
                        </Form>
                        <Form action={act.mergeDevice} confirm={`Gộp lịch sử vào ${d.name} rồi xoá Device cũ?`}>
                          <input type="hidden" name="device" value={d.id} />
                          <select name="from" aria-label={`Gộp lịch sử từ Device vào ${d.name}`} required defaultValue="">
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
                          <button className="btn">Gộp</button>
                        </Form>
                        {!d.revoked_at && (
                          <Form action={act.revokeDevice} confirm={`Thu hồi ${d.name}? Device không thể kết nối lại.`}>
                            <input type="hidden" name="device" value={d.id} />
                            <button className="btn danger">Thu hồi</button>
                          </Form>
                        )}
                        <Form action={act.deleteDevice} confirm={`Xoá ${d.name} cùng lịch sử của nó?`}>
                          <input type="hidden" name="device" value={d.id} />
                          <button className="btn danger">Xoá</button>
                        </Form>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        </div>
      </main>
    </>
  );
}
