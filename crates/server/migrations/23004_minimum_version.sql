-- The Minimum Version (empty = none) and why a Device's last self-update toward it failed.
ALTER TABLE server_ca ADD COLUMN minimum_version TEXT NOT NULL DEFAULT '';
ALTER TABLE devices ADD COLUMN update_error TEXT;
