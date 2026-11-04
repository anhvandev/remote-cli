-- Revoked Devices can no longer connect (their certificate is refused).
ALTER TABLE devices ADD COLUMN revoked_at TIMESTAMPTZ;

-- Deleting a Device keeps the record that its Enrollment Code was used.
ALTER TABLE enrollment_codes DROP CONSTRAINT enrollment_codes_device_id_fkey,
    ADD FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE SET NULL;
