-- The API Address: the Dashboard's public https:// URL, learned from an Admin's visit or set by an Admin.
ALTER TABLE server_ca ADD COLUMN api_address TEXT NOT NULL DEFAULT '';
