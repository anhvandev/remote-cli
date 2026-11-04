-- Migration versions are <ticket NN><seq 3 digits>, e.g. ticket 02 uses 02001, 02002...
-- so parallel tickets never pick the same number.

CREATE TABLE server_ca (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    cert_pem TEXT NOT NULL,
    key_pem TEXT NOT NULL
);

CREATE TABLE devices (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    cert_fingerprint BYTEA NOT NULL UNIQUE,
    enrolled_at TIMESTAMPTZ NOT NULL,
    -- When the Agent's last connection ended; NULL while connected or never connected.
    last_seen_at TIMESTAMPTZ
);

CREATE TABLE enrollment_codes (
    code_hash BYTEA PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    device_id UUID REFERENCES devices(id)
);
