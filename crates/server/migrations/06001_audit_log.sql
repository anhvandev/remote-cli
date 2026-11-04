-- Audit Log: one row per Action. No FKs so entries outlive deleted Devices and users.
CREATE TABLE audit_log (
    id BIGSERIAL PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL,
    user_id UUID NOT NULL,
    user_name TEXT NOT NULL,
    device_id UUID NOT NULL,
    action TEXT NOT NULL,
    params JSONB NOT NULL,
    result JSONB NOT NULL
);
CREATE INDEX audit_log_at ON audit_log (at);
CREATE INDEX audit_log_device_at ON audit_log (device_id, at);
