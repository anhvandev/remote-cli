"use client";
import { useActionState } from "react";
import { signIn } from "../actions";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export default function SignInPage() {
  const [state, action, pending] = useActionState(signIn, {});
  return (
    // The same ink column as the signed-in sidebar, so signing in reads as entering the console.
    <main className="grid min-h-screen w-full md:grid-cols-[15rem_minmax(0,1fr)]">
      <div className="bg-ink px-5 py-5 text-lg font-extrabold tracking-tight text-paper">Remote CLI</div>
      <div className="flex items-center px-5 py-10 md:px-16">
        <form action={action} className="w-full max-w-sm">
          <div className="mb-6 border-b-2 border-ink pb-4">
            <p className="text-sm font-medium text-muted-foreground">Dashboard</p>
            <h2 className="mt-1 text-3xl font-extrabold tracking-tight">Đăng nhập</h2>
          </div>
          <div className="grid gap-4">
            <div className="grid gap-1.5">
              <Label htmlFor="name" className="font-medium">
                Tên đăng nhập
              </Label>
              <Input id="name" name="name" autoComplete="username" required />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="password" className="font-medium">
                Mật khẩu
              </Label>
              <Input id="password" name="password" type="password" autoComplete="current-password" required />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="totp" className="font-medium">
                Mã TOTP (ứng dụng xác thực)
              </Label>
              <Input
                id="totp"
                name="totp"
                className="text-base"
                inputMode="numeric"
                autoComplete="one-time-code"
                pattern="[0-9]{6}"
                maxLength={6}
                required
              />
            </div>
            {state.error && (
              <div className="border-2 border-destructive px-3 py-2 text-sm text-destructive" role="alert">
                {state.error}
              </div>
            )}
            <Button type="submit" disabled={pending} aria-busy={pending} className="mt-2">
              Đăng nhập
            </Button>
          </div>
        </form>
      </div>
    </main>
  );
}
