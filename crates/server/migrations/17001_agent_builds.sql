-- Signed Agent builds and per-Device rollout state.
CREATE TABLE agent_builds (
    version TEXT NOT NULL,
    target TEXT NOT NULL,
    signature BYTEA NOT NULL,
    build BYTEA NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL,
    uploaded_by TEXT NOT NULL,
    PRIMARY KEY (version, target)
);

ALTER TABLE devices
    ADD COLUMN agent_version TEXT,
    ADD COLUMN agent_target TEXT,
    -- Version an admin rolled out to this Device; NULL = none.
    ADD COLUMN update_version TEXT,
    -- Why the Device could not install update_version; NULL = not failed.
    ADD COLUMN update_error TEXT;
