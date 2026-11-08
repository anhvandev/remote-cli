"use client";
import { useActionState } from "react";
import { ShieldCheck } from "lucide-react";
import { signIn } from "../actions";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export default function SignInPage() {
  const [state, action, pending] = useActionState(signIn, {});
  return (
    <main className="relative flex min-h-screen w-full items-center justify-center px-4">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(60%_50%_at_50%_0%,rgba(91,140,255,0.12),transparent)]" />
      <form action={action} className="relative w-full max-w-sm rounded-xl border border-border bg-card p-8 shadow-2xl shadow-black/40">
        <div className="mb-7 flex flex-col items-center gap-3 text-center">
          <span className="grid size-10 place-items-center rounded-lg bg-primary text-primary-foreground">
            <ShieldCheck className="size-5" />
          </span>
          <div>
            <h2 className="text-lg font-semibold tracking-tight">Đăng nhập</h2>
            <p className="text-sm text-muted-foreground">Remote CLI · Dashboard</p>
          </div>
        </div>
        <div className="grid gap-4">
          <div className="grid gap-1.5">
            <Label htmlFor="name" className="text-muted-foreground">
              Tên đăng nhập
            </Label>
            <Input id="name" name="name" autoComplete="username" required />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="password" className="text-muted-foreground">
              Mật khẩu
            </Label>
            <Input id="password" name="password" type="password" autoComplete="current-password" required />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="totp" className="text-muted-foreground">
              Mã TOTP (ứng dụng xác thực)
            </Label>
            <Input
              id="totp"
              name="totp"
              className="text-center font-mono text-base tracking-[0.3em]"
              inputMode="numeric"
              autoComplete="one-time-code"
              pattern="[0-9]{6}"
              maxLength={6}
              required
            />
          </div>
          {state.error && (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">
              {state.error}
            </div>
          )}
          <Button type="submit" disabled={pending} className="mt-1">
            Đăng nhập
          </Button>
        </div>
      </form>
    </main>
  );
}
