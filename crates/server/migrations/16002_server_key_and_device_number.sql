-- Server Key (rk_<CA fingerprint>_<secret>) replaces Enrollment Codes: a Device registers by presenting it.
ALTER TABLE server_ca ADD COLUMN server_key_secret TEXT NOT NULL DEFAULT '';

-- The 9-digit Device ID the CLI and Dashboard address a Device by; the UUID stays the storage key.
ALTER TABLE devices ADD COLUMN device_number TEXT NOT NULL DEFAULT '';
ALTER TABLE devices ALTER COLUMN device_number DROP DEFAULT;
ALTER TABLE devices ADD CONSTRAINT devices_device_number_key UNIQUE (device_number);

-- The audit log keeps the 9-digit Device ID so entries outlive a deleted Device.
ALTER TABLE audit_log ADD COLUMN device_number TEXT;

-- Enrollment Codes are gone (after 16001 dropped their foreign key to devices).
DROP TABLE enrollment_codes;
