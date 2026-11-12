-- Agent Builds are unsigned and the Latest Build is the highest semver version (ADR 0008): no signatures, no
-- rollouts, no per-Device rollout state.
ALTER TABLE agent_builds
    DROP COLUMN signature,
    DROP COLUMN rolled_out_at;
ALTER TABLE devices
    DROP COLUMN update_version,
    DROP COLUMN update_error;
