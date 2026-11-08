import { History } from "../history";
import { getDevice, devicePath } from "../device-data";

export default async function HistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ range?: string }>;
}) {
  const { id } = await params;
  const { range } = await searchParams;
  const d = await getDevice(id);
  return <History path={devicePath(id)} range={range} ramBytes={d.inventory?.ram_bytes} />;
}
