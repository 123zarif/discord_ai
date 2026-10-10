-- Migration 004: Discord messages storage with vector embeddings for semantic search and style modeling

CREATE TABLE IF NOT EXISTS discord_messages (
    id BIGSERIAL PRIMARY KEY,
    message_id BIGINT NOT NULL UNIQUE,
    channel_id BIGINT NOT NULL,
    guild_id BIGINT NOT NULL DEFAULT 0,
    author_id BIGINT NOT NULL,
    author_name VARCHAR(128) NOT NULL,
    content TEXT NOT NULL,
    is_target_user BOOLEAN NOT NULL DEFAULT FALSE,
    reply_to_message_id BIGINT,
    message_timestamp TIMESTAMPTZ NOT NULL,
    embedding vector(384),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for chronological channel timeline queries
CREATE INDEX IF NOT EXISTS idx_discord_messages_channel
ON discord_messages (channel_id, message_timestamp DESC);

-- Index for user-specific queries
CREATE INDEX IF NOT EXISTS idx_discord_messages_author
ON discord_messages (author_id);

-- Index for filtering target user messages
CREATE INDEX IF NOT EXISTS idx_discord_messages_target_user
ON discord_messages (is_target_user);

-- HNSW index for fast approximate nearest neighbor cosine vector similarity search
CREATE INDEX IF NOT EXISTS idx_discord_messages_embedding_hnsw
ON discord_messages USING hnsw (embedding vector_cosine_ops);

