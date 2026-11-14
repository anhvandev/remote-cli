"use client";
import Link from "next/link";
import { useSelectedLayoutSegment } from "next/navigation";

const TABS = [
  ["", "Tổng quan"],
  ["screen", "Màn hình"],
  ["terminal", "Terminal"],
  ["history", "Lịch sử"],
] as const;

export function Tabs({ device, headless }: { device: string; headless?: boolean }) {
  // The tab slug below this layout; null on the Device's root (Tổng quan).
  const active = useSelectedLayoutSegment() ?? "";
  const tabs = headless ? TABS.filter(([slug]) => slug !== "screen") : TABS;
  return (
    <div className="flex overflow-x-auto border-b-2 border-ink">
      {tabs.map(([slug, name]) => (
        <Link
          key={slug}
          href={`/devices/${device}${slug ? `/${slug}` : ""}`}
          aria-current={active === slug ? "page" : undefined}
          className={`px-4 py-2 text-sm font-medium whitespace-nowrap transition-colors duration-(--dur-micro) focus-visible:outline-2 focus-visible:-outline-offset-4 focus-visible:outline-ring ${
            active === slug ? "bg-ink text-paper" : "text-muted-foreground hover:bg-muted hover:text-foreground"
          }`}
        >
          {name}
        </Link>
      ))}
      {headless && <span className="px-4 py-2 text-sm whitespace-nowrap text-muted-foreground">Device không có màn hình.</span>}
    </div>
  );
}
