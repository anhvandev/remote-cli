import { getDevice } from "../device-data";
import { LiveView } from "../live-view";
import { LockCard } from "../control";

export default async function ScreenPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const d = await getDevice(id);
  return (
    <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
      <LiveView device={d.id} name={d.name} />
      <LockCard />
    </div>
  );
}
