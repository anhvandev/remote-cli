-- Dashboard sign-in: password + TOTP per user; sessions are API tokens that expire.
ALTER TABLE users
    ADD COLUMN password_hash TEXT,
    ADD COLUMN totp_secret BYTEA,
    ADD COLUMN totp_last_step BIGINT;

ALTER TABLE api_tokens ADD COLUMN expires_at TIMESTAMPTZ;
