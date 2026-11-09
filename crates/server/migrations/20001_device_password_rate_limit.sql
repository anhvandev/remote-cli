-- Wrong Device Passwords are rate-limited per (Device, IP): 5 failures lock the pair out for a minute, and each
-- later lockout doubles up to an hour. `device` is the Device's UUID when it exists, else the presented ID, so every
-- way of naming a Device shares one bucket and unknown IDs are counted too.
CREATE TABLE device_password_failures (
    device TEXT NOT NULL,
    ip TEXT NOT NULL,
    failures INT NOT NULL DEFAULT 0,
    lockouts INT NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    seen_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (device, ip)
);
CREATE INDEX device_password_failures_seen_at ON device_password_failures (seen_at);
