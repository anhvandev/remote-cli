-- When a build was last rolled out to every Device: the newest such build per target is the fleet's build, which
-- new installs download and Controller-only machines upgrade to. NULL = never rolled out to the whole fleet.
ALTER TABLE agent_builds ADD COLUMN rolled_out_at TIMESTAMPTZ;
