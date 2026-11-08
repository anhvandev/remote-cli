"use client";
// Header reminder on every tab: you hold the Control Lock, others are blocked until you let go.
import { Button } from "@/components/ui/button";
import { useHolding, useRelease } from "./control";

export function ControlReminder() {
  const holding = useHolding();
  const release = useRelease();
  if (!holding) return null;
  return (
    <p className="flex items-center gap-2 text-sm">
      <span>Bạn đang giữ Control Lock</span>
      <Button size="sm" variant="secondary" onClick={() => release()}>
        Nhả
      </Button>
    </p>
  );
}
