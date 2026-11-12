-- Install packages move to the file storage service, stored at `packages/<sha256>`; Postgres keeps only metadata.
-- No data moves: the only deployment has no uploaded packages (any row would have lost its bytes anyway).
DELETE FROM install_packages;
ALTER TABLE install_packages
    DROP COLUMN package,
    ADD COLUMN sha256 TEXT NOT NULL,
    ADD COLUMN size BIGINT NOT NULL;
