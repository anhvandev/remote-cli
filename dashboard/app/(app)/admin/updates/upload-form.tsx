"use client";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { Result } from "../actions";
import { Form } from "../form";

const str = (form: FormData, key: string) => String(form.get(key) ?? "");
const seg = encodeURIComponent;

// `<os>-<arch>` as the Agent reports it (std::env::consts::OS / ARCH).
const TARGETS = ["linux-x86_64", "linux-aarch64", "windows-x86_64", "windows-aarch64", "macos-aarch64", "macos-x86_64"];

/** Version and target from a file named like `rcli-0.3.0-linux-x86_64` (an `.exe` suffix allowed). */
function fromName(name: string) {
  const m = /^rcli-(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)-([a-z]+-[a-z0-9_]+?)(?:\.exe)?$/.exec(name);
  return m && TARGETS.includes(m[2]) ? { version: m[1], target: m[2] } : null;
}

// A route handler, not a server action: a server action reads the whole build into memory.
async function upload(_: Result, form: FormData): Promise<Result> {
  const version = str(form, "version");
  const file = form.get("build");
  if (!(file instanceof File) || file.size === 0) return { error: "bad_request: chọn file build" };
  const url = `/api/agent-builds/${seg(version)}/${seg(str(form, "target"))}`;
  const res = await fetch(url, { method: "PUT", headers: { "content-type": "application/octet-stream" }, body: file });
  if (res.ok) return { message: `Đã tải lên ${version}` };
  const body = await res.json().catch(() => null);
  return { error: `${body?.error?.code ?? "http_error"}: ${body?.error?.message ?? res.statusText}` };
}

/** The Updates page's Agent Build upload form: file, version and target, prefilled from the file name. */
export function UploadForm() {
  const [version, setVersion] = useState("");
  const [target, setTarget] = useState("");
  const router = useRouter();
  return (
    <Form
      action={async (prev, form) => {
        const r = await upload(prev, form);
        if (r.message) router.refresh(); // lists the new build
        return r;
      }}
      className="grid gap-2"
    >
      <Input
        type="file"
        name="build"
        aria-label="File build"
        required
        className="h-9 py-1.5"
        onChange={(e) => {
          const parsed = fromName(e.target.files?.[0]?.name ?? "");
          if (parsed) {
            setVersion(parsed.version);
            setTarget(parsed.target);
          }
        }}
      />
      <div className="flex flex-wrap gap-2">
        <Input
          name="version"
          aria-label="Phiên bản"
          placeholder="phiên bản (X.Y.Z)"
          required
          className="h-8 w-32"
          value={version}
          onChange={(e) => setVersion(e.target.value)}
        />
        <Select name="target" required value={target} onValueChange={setTarget}>
          <SelectTrigger aria-label="Hệ điều hành" size="sm" className="w-44">
            <SelectValue placeholder="chọn hệ điều hành" />
          </SelectTrigger>
          <SelectContent>
            {TARGETS.map((t) => (
              <SelectItem key={t} value={t}>
                {t}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div>
        <Button size="sm" variant="secondary">
          Tải lên
        </Button>
      </div>
    </Form>
  );
}
