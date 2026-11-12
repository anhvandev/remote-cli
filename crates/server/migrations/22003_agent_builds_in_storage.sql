-- Agent builds move to the file storage service, stored at `agent-builds/<sha256>`; Postgres keeps only metadata.
-- No data moves: the only deployment has no uploaded builds. Devices pointing at a deleted version get an update
-- error ("no build of ...") as with any missing build.
DELETE FROM agent_builds;
ALTER TABLE agent_builds
    DROP COLUMN build,
    ADD COLUMN sha256 TEXT NOT NULL,
    ADD COLUMN size BIGINT NOT NULL;
