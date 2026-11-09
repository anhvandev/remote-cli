-- Server Settings: the Server Address (replacing RCLI_AGENT_HOSTNAMES) is stored here and edited by Admins.
ALTER TABLE server_ca ADD COLUMN server_address TEXT NOT NULL DEFAULT '';
