"use client";
import type { ReactNode } from "react";
import type { Result } from "../actions";
import { Form } from "../form";

const str = (form: FormData, key: string) => String(form.get(key) ?? "");
const seg = encodeURIComponent;

// A route handler, not a server action: a server action reads the whole build into memory.
async function upload(_: Result, form: FormData): Promise<Result> {
  const version = str(form, "version");
  const file = form.get("build");
  if (!(file instanceof File) || file.size === 0) return { error: "bad_request: chọn file build" };
  const url = `/api/agent-builds/${seg(version)}/${seg(str(form, "target"))}?signature=${seg(str(form, "signature"))}`;
  const res = await fetch(url, { method: "PUT", headers: { "content-type": "application/octet-stream" }, body: file });
  if (res.ok) return { message: `Đã tải lên ${version}` };
  const body = await res.json().catch(() => null);
  return { error: `${body?.error?.code ?? "http_error"}: ${body?.error?.message ?? res.statusText}` };
}

/** The Updates page's Agent build upload form. */
export function UploadForm({ children }: { children: ReactNode }) {
  return (
    <Form action={upload} className="grid gap-2">
      {children}
    </Form>
  );
}
