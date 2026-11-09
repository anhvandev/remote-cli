import { notFound } from "next/navigation";
import { ApiError } from "@/lib/api";
import { Refresh } from "../../../refresh";
import { Status } from "../../../status";
import { ControlProvider } from "./control";
import { ControlReminder } from "./control-reminder";
import { getDevice, getLock, getMe, uptime } from "./device-data";
import { PowerButtons } from "./power";
import { Tabs } from "./tabs";

/** Shared by the four Device tabs: the header, the tab bar, and the Control Lock watch that outlives a tab switch. */
export default async function DeviceLayout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const [d, lock, me] = await Promise.all([getDevice(id), getLock(id), getMe()]).catch((e) => {
    if (e instanceof ApiError && e.status === 404) notFound();
    throw e;
  });
  const inv = d.inventory;
  const r = d.resources;
  // Header meta line: OS, hostname/IP, agent version, uptime, signed-in user.
  const ip = inv?.network_interfaces.flatMap((n) => n.ips)[0];
  const meta = [
    `${inv?.os} ${inv?.os_version}`.trim(),
    ip,
    d.agent_version && `Agent ${d.agent_version}`,
    r?.uptime_secs !== undefined && `uptime ${uptime(r.uptime_secs)}`,
    r?.logged_in_user && `user: ${r.logged_in_user}`,
  ].filter(Boolean);

  return (
    <>
      <Refresh />
      <ControlProvider device={d.id} me={me.name} initial={lock}>
        <div className="flex flex-wrap items-start justify-between gap-4 border-b border-border pb-5">
          <div className="min-w-0">
            <h2 className="flex flex-wrap items-center gap-2.5 text-xl font-semibold tracking-tight">
              {d.name}
              <span className="text-sm font-normal">
                <Status status={d.status} wakeError={d.wake_error} />
              </span>
            </h2>
            <p className="mt-1 truncate text-sm text-muted-foreground">{meta.join(" · ")}</p>
          </div>
          <div className="flex flex-col items-end gap-2">
            <PowerButtons device={d.id} name={d.name} status={d.status} me={me.name} admin={me.role === "admin"} />
            <ControlReminder />
          </div>
        </div>
        <Tabs device={d.id} />
        <div className="mt-5">{children}</div>
      </ControlProvider>
    </>
  );
}
