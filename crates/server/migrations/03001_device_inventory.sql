-- Latest inventory reported by each Device's Agent (protobuf-encoded rcli.agent.v1.Inventory).
CREATE TABLE device_inventory (
    device_id UUID PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    inventory BYTEA NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
