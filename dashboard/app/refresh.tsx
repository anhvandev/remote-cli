"use client";
import { useRouter } from "next/navigation";
import { useEffect } from "react";

/** Re-renders the server components every few seconds so Device Status stays current. */
export function Refresh({ everyMs = 5000 }: { everyMs?: number }) {
  const router = useRouter();
  useEffect(() => {
    const t = setInterval(() => router.refresh(), everyMs);
    return () => clearInterval(t);
  }, [router, everyMs]);
  return null;
}
