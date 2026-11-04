-- Idle timer (10 min from the holder's last Action) and disconnect grace (60 s from when the holder's last watch closed).
ALTER TABLE control_locks ADD COLUMN last_action_at TIMESTAMPTZ;
UPDATE control_locks SET last_action_at = acquired_at;
ALTER TABLE control_locks ALTER COLUMN last_action_at SET NOT NULL;
ALTER TABLE control_locks ADD COLUMN disconnected_at TIMESTAMPTZ;
