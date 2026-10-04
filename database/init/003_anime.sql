-- Migration 003: Anime cache, watchlist, and peer recommendations

CREATE TABLE IF NOT EXISTS anime_cache (
    id INT PRIMARY KEY,
    romaji_title TEXT NOT NULL,
    english_title TEXT,
    cover_url TEXT,
    banner_url TEXT,
    genres TEXT[] NOT NULL DEFAULT '{}',
    episodes INT,
    average_score INT,
    synopsis TEXT,
    site_url TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_anime_watchlist (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    anime_id INT NOT NULL REFERENCES anime_cache(id) ON DELETE CASCADE,
    status VARCHAR(20) NOT NULL DEFAULT 'planning', -- 'watching', 'planning', 'completed', 'on_hold', 'dropped'
    progress INT NOT NULL DEFAULT 0,
    score INT, -- 1 to 10
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_user_anime_entry UNIQUE (user_id, anime_id)
);

CREATE INDEX IF NOT EXISTS idx_user_watchlist_lookup
ON user_anime_watchlist (user_id, status);

CREATE TABLE IF NOT EXISTS user_anime_recommendations (
    id BIGSERIAL PRIMARY KEY,
    sender_id BIGINT NOT NULL,
    recipient_id BIGINT NOT NULL,
    anime_id INT NOT NULL REFERENCES anime_cache(id) ON DELETE CASCADE,
    note TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending', -- 'pending', 'added', 'dismissed'
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_user_recommendations_recipient
ON user_anime_recommendations (recipient_id, status);
