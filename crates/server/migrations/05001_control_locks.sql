-- One Control Lock per Device. holder_id has no FK: the bootstrap admin (nil id) is not a users row.
CREATE TABLE control_locks (
    device_id UUID PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    holder_id UUID NOT NULL,
    holder_name TEXT NOT NULL,
    acquired_at TIMESTAMPTZ NOT NULL
);
