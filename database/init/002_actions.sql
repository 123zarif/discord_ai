-- Create table for tracking user interactions and action counts
CREATE TABLE IF NOT EXISTS action_counts (
    id BIGSERIAL PRIMARY KEY,
    guild_id BIGINT NOT NULL DEFAULT 0,
    user_id BIGINT NOT NULL,
    target_id BIGINT NOT NULL,
    action_type VARCHAR(32) NOT NULL,
    count BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_guild_user_target_action UNIQUE (guild_id, user_id, target_id, action_type)
);

CREATE INDEX IF NOT EXISTS idx_action_counts_lookup 
ON action_counts (guild_id, user_id, target_id, action_type);
