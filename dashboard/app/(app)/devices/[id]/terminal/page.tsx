import { getDevice, getMe } from "../device-data";
import { Terminal } from "../terminal";

export default async function TerminalPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const [d, me] = await Promise.all([getDevice(id), getMe()]);
  return <Terminal device={d.id} admin={me.role === "admin"} />;
}
