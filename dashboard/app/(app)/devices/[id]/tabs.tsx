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
    <div className="mt-4 flex gap-1 border-b border-border">
      {tabs.map(([slug, name]) => (
        <Link
          key={slug}
          href={`/devices/${device}${slug ? `/${slug}` : ""}`}
          aria-current={active === slug ? "page" : undefined}
          className={`-mb-px border-b-2 px-3 py-2 text-sm transition-colors ${
            active === slug
              ? "border-primary font-medium text-foreground"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          {name}
        </Link>
      ))}
      {headless && <span className="px-3 py-2 text-sm text-muted-foreground">Device không có màn hình.</span>}
    </div>
  );
}
