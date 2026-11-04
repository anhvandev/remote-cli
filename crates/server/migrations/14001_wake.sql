-- When the last Wake was sent; cleared when the Agent connects. Drives Waking / "Wake failed".
ALTER TABLE devices ADD COLUMN wake_requested_at TIMESTAMPTZ;
