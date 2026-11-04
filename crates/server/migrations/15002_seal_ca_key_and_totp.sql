-- With RCLI_MASTER_KEY the CA private key and TOTP secrets are stored sealed (AES-256-GCM, like unlock passwords).
-- Plaintext rows from before (or from Servers without a master key) are sealed on the next start with a key.
ALTER TABLE server_ca ALTER COLUMN key_pem DROP NOT NULL, ADD COLUMN key_sealed BYTEA;
ALTER TABLE users ADD COLUMN totp_sealed BOOLEAN NOT NULL DEFAULT FALSE;
