-- Migration 005: Media cache, movie & TV series watchlist, and unified recommendations

CREATE TABLE IF NOT EXISTS media_cache (
    imdb_id VARCHAR(32) PRIMARY KEY,
    media_type VARCHAR(16) NOT NULL, -- 'movie' or 'series'
    title TEXT NOT NULL,
    year VARCHAR(32),
    poster_url TEXT,
    backdrop_url TEXT,
    genres TEXT[] NOT NULL DEFAULT '{}',
    imdb_rating VARCHAR(16),
    runtime VARCHAR(32),
    synopsis TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_media_watchlist (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    imdb_id VARCHAR(32) NOT NULL REFERENCES media_cache(imdb_id) ON DELETE CASCADE,
    status VARCHAR(20) NOT NULL DEFAULT 'planning', -- 'watching', 'planning', 'completed', 'on_hold', 'dropped'
    progress INT NOT NULL DEFAULT 0,
    score INT, -- 1 to 10
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_user_media_entry UNIQUE (user_id, imdb_id)
);

CREATE INDEX IF NOT EXISTS idx_user_media_watchlist_lookup
ON user_media_watchlist (user_id, status);

CREATE TABLE IF NOT EXISTS user_unified_recommendations (
    id BIGSERIAL PRIMARY KEY,
    sender_id BIGINT NOT NULL,
    recipient_id BIGINT NOT NULL,
    media_type VARCHAR(16) NOT NULL, -- 'anime', 'movie', 'series'
    media_id VARCHAR(64) NOT NULL, -- AniList ID or Cinemeta IMDb ID
    title TEXT NOT NULL,
    poster_url TEXT,
    note TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending', -- 'pending', 'added', 'dismissed'
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_user_unified_recommendations_recipient
ON user_unified_recommendations (recipient_id, status);

