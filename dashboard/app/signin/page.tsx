"use client";
import { useActionState } from "react";
import { signIn } from "../actions";

export default function SignInPage() {
  const [state, action, pending] = useActionState(signIn, {});
  return (
    <main>
      <form action={action} className="card login">
        <h2>Đăng nhập</h2>
        <div className="grid">
          <label className="muted">
            Tên đăng nhập
            <input name="name" autoComplete="username" required />
          </label>
          <label className="muted">
            Mật khẩu
            <input name="password" type="password" autoComplete="current-password" required />
          </label>
          <label className="muted">
            Mã TOTP (ứng dụng xác thực)
            <input
              name="totp"
              className="otp"
              inputMode="numeric"
              autoComplete="one-time-code"
              pattern="[0-9]{6}"
              maxLength={6}
              required
            />
          </label>
          {state.error && (
            <div className="error" role="alert">
              {state.error}
            </div>
          )}
          <button className="btn primary" disabled={pending}>
            Đăng nhập
          </button>
        </div>
      </form>
    </main>
  );
}
