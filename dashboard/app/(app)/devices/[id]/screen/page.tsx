import { getDevice } from "../device-data";
import { LiveView } from "../live-view";
import { LockCard } from "../control";
import { Placeholder } from "../placeholder";

export default async function ScreenPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const d = await getDevice(id);
  if (d.inventory?.displays.length === 0) {
    return <p className="text-sm text-muted-foreground">Device không có màn hình.</p>;
  }
  if (d.status !== "Online") return <Placeholder device={d.id} status={d.status} />;
  return (
    <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
      <LiveView device={d.id} name={d.name} />
      <LockCard />
    </div>
  );
}
