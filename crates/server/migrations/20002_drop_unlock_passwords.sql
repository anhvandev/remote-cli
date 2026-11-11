-- Unlock passwords moved to each Device's Secret Store (easy-setup ticket 09); the Server's vault is dropped.
-- 15001 stays so a database that already applied it still matches the migration history.
DROP TABLE unlock_passwords;
