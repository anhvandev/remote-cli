-- Secrets Vault: one unlock password per Device, AES-256-GCM under the server master key.
-- sealed = 12-byte nonce || ciphertext || tag; the Device id is the AAD, so a row can't be moved to another Device.
CREATE TABLE unlock_passwords (
    device_id UUID PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    sealed BYTEA NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
