-- Every User may do everything (ADR 0009); former Operators need no migration step.
ALTER TABLE users DROP COLUMN role;
