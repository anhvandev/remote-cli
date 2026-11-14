"use client";
import { useActionState, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import type { Result } from "./actions";

/** A form posting to an admin server action; shows its one-time result (new token, code...) or error. */
export function Form({
  action,
  confirm,
  children,
  label,
  className,
}: {
  action: (prev: Result, form: FormData) => Promise<Result>;
  /** Asks before submitting (destructive actions). */
  confirm?: string;
  children: ReactNode;
  label?: string;
  className?: string;
}) {
  const [state, run, pending] = useActionState(action, {});
  return (
    <form
      action={run}
      aria-label={label}
      className={cn("flex flex-wrap items-center gap-2", className)}
      onSubmit={(e) => {
        if (confirm && !window.confirm(confirm)) e.preventDefault();
      }}
    >
      <fieldset disabled={pending} className="contents">
        {children}
      </fieldset>
      {state.message && (
        <output role="status" className="basis-full text-xs font-medium break-all">
          {state.message}
        </output>
      )}
      {state.error && (
        <span role="alert" className="basis-full text-sm text-destructive">
          {state.error}
        </span>
      )}
    </form>
  );
}
