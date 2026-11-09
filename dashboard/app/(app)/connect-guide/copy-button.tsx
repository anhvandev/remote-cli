"use client";
import { useState } from "react";
import { Button } from "@/components/ui/button";

/** Copies `value` to the clipboard and confirms for a moment. */
export function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      size="sm"
      variant="secondary"
      className="shrink-0"
      onClick={() => {
        copy(value);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      }}
    >
      {copied ? "Đã sao chép" : "Sao chép"}
    </Button>
  );
}

// `navigator.clipboard` is missing on plain http (non-localhost); the execCommand fallback keeps the button working there.
function copy(value: string) {
  if (navigator.clipboard) return navigator.clipboard.writeText(value);
  const t = document.createElement("textarea");
  t.value = value;
  document.body.appendChild(t);
  t.select();
  document.execCommand("copy");
  t.remove();
}
