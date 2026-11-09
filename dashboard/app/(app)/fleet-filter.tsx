"use client";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import type { DeviceStatus } from "@/lib/api";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

/** Writes the fleet filters to the URL so Back, refresh and shared links keep them. */
export function FleetFilter() {
  const params = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  const set = (key: string, value: string) => {
    const next = new URLSearchParams(params);
    if (value) next.set(key, value);
    else next.delete(key);
    router.replace(`${pathname}${next.size ? `?${next}` : ""}`);
  };
  return (
    <div className="mb-4 flex flex-wrap gap-2">
      <Input
        type="search"
        aria-label="Tìm Device"
        placeholder="Tìm theo tên…"
        defaultValue={params.get("q") ?? ""}
        onChange={(e) => set("q", e.target.value)}
        className="max-w-xs"
      />
      <Select value={params.get("status") ?? "All"} onValueChange={(v) => set("status", v === "All" ? "" : v)}>
        <SelectTrigger aria-label="Trạng thái" className="w-36">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {(["All", "Online", "Offline", "Waking"] satisfies ("All" | DeviceStatus)[]).map((s) => (
            <SelectItem key={s} value={s}>
              {s === "All" ? "Tất cả" : s}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
