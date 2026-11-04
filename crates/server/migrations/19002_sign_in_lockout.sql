-- Sign-in lockout: consecutive failed sign-ins per user, and until when they are locked out.
ALTER TABLE users
    ADD COLUMN failed_sign_ins INT NOT NULL DEFAULT 0,
    ADD COLUMN sign_in_locked_until TIMESTAMPTZ;
