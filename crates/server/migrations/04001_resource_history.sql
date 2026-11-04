-- Resource history. `resources` = complete merged sample as the API's JSON (rcli.agent.v1.Resources).
-- Raw 5-second samples, kept 7 days. `rolled_up` = already counted in resource_rollups.
CREATE TABLE resource_samples (
    device_id UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    at TIMESTAMPTZ NOT NULL,
    resources JSONB NOT NULL,
    rolled_up BOOLEAN NOT NULL DEFAULT false
);
CREATE INDEX resource_samples_device_at ON resource_samples (device_id, at);
CREATE INDEX resource_samples_pending ON resource_samples (at) WHERE NOT rolled_up;

-- 1-minute rollups (numbers averaged, other values = last in the minute), kept 90 days.
CREATE TABLE resource_rollups (
    device_id UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    minute TIMESTAMPTZ NOT NULL,
    resources JSONB NOT NULL,
    PRIMARY KEY (device_id, minute)
);
