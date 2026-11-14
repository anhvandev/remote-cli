"use client";
import { useEffect, useState } from "react";

/** Red banner while the Dashboard is reached over http: the one-line install can be tampered with (ticket 18).
 * `claimedHttps` is the proxy's X-Forwarded-Proto. shortcut: "https" there suppresses the banner even over http
 * (an on-path attacker could set it, but can rewrite the page anyway) — it makes both states testable; upgrade if
 * the Dashboard ever serves TLS itself. */
export function HttpWarning({ claimedHttps }: { claimedHttps: boolean }) {
  const [insecure, setInsecure] = useState(false);
  useEffect(() => {
    setInsecure(window.location.protocol === "http:" && !claimedHttps);
  }, [claimedHttps]);
  if (!insecure) return null;
  return (
    <div role="alert" aria-label="Cảnh báo http" className="mb-5 border-2 border-destructive px-5 py-3.5 text-sm font-medium text-destructive">
      Dashboard đang chạy qua http: lệnh cài đặt một dòng và Server Key truyền không mã hoá, có thể bị giả mạo trên
      đường truyền. Hãy đặt TLS (https) trước Dashboard.
    </div>
  );
}
