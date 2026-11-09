-- Device Password: the Server keeps only an argon2 hash; the Device keeps the password itself.
ALTER TABLE devices ADD COLUMN device_password_hash TEXT NOT NULL DEFAULT '';
