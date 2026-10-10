pub mod actions;
pub mod anime;
pub mod health;
pub mod instagram;
pub mod media;
pub mod messages;

use std::collections::HashSet;
use std::time::Duration;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use crate::anilist::AnimeMedia;
use crate::cinemeta::CinemetaMedia;
use crate::error::{DbError, Result};
pub use anime::{RecommendationDetailedEntry, WatchlistDetailedEntry};
pub use health::DatabaseHealthReport;
pub use instagram::InstagramSender;
pub use media::{MediaWatchlistDetailedEntry, UnifiedRecommendationEntry, UnifiedWatchlistEntry};
pub use messages::NewDiscordMessage;

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Connects to PostgreSQL using configured connection pooling options.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .min_connections(2)
            .acquire_timeout(Duration::from_secs(5))
            .idle_timeout(Duration::from_secs(600))
            .connect(database_url)
            .await
            .map_err(DbError::ConnectionFailed)?;

        Ok(Self { pool })
    }

    /// Accessor for the PostgreSQL connection pool.
    #[allow(dead_code)]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Performs an active health check verifying connection and pgvector extension status.
    pub async fn health_check(&self) -> Result<DatabaseHealthReport> {
        health::check_health(&self.pool).await.map_err(Into::into)
    }

    /// Initializes all database schemas and tables needed by the bot.
    pub async fn init_tables(&self) -> Result<()> {
        actions::init_tables(&self.pool).await?;
        anime::init_tables(&self.pool).await?;
        instagram::init_tables(&self.pool).await?;
        messages::init_tables(&self.pool).await?;
        media::init_tables(&self.pool).await?;
        Ok(())
    }

    /// Increments user interaction count for a specific action type.
    pub async fn increment_action_count(
        &self,
        guild_id: u64,
        user_id: u64,
        target_id: u64,
        action_type: &str,
    ) -> Result<i64> {
        actions::increment_action_count(&self.pool, guild_id, user_id, target_id, action_type)
            .await
            .map_err(Into::into)
    }

    /// Decrements user interaction count for a specific action type (e.g. on rejection).
    pub async fn decrement_action_count(
        &self,
        guild_id: u64,
        user_id: u64,
        target_id: u64,
        action_type: &str,
    ) -> Result<i64> {
        actions::decrement_action_count(&self.pool, guild_id, user_id, target_id, action_type)
            .await
            .map_err(Into::into)
    }

    /// Retrieves the current interaction count between user and target for a given action type.
    pub async fn get_action_count(
        &self,
        guild_id: u64,
        user_id: u64,
        target_id: u64,
        action_type: &str,
    ) -> Result<i64> {
        actions::get_action_count(&self.pool, guild_id, user_id, target_id, action_type)
            .await
            .map_err(Into::into)
    }

    /// Caches an anime in the database.
    pub async fn upsert_anime_cache(&self, anime: &AnimeMedia) -> Result<()> {
        anime::upsert_anime_cache(&self.pool, anime).await?;
        Ok(())
    }

    /// Adds or updates a user's watchlist entry.
    pub async fn upsert_watchlist_entry(
        &self,
        user_id: u64,
        anime_id: i32,
        status: &str,
        progress: Option<i32>,
        score: Option<i32>,
    ) -> Result<()> {
        anime::upsert_watchlist_entry(&self.pool, user_id, anime_id, status, progress, score).await?;
        Ok(())
    }

    /// Retrieves a user's watchlist entries.
    pub async fn get_user_watchlist(
        &self,
        user_id: u64,
        status_filter: Option<&str>,
    ) -> Result<Vec<WatchlistDetailedEntry>> {
        anime::get_user_watchlist(&self.pool, user_id, status_filter).await.map_err(Into::into)
    }

    /// Removes an anime from a user's watchlist.
    pub async fn remove_from_watchlist(&self, user_id: u64, anime_id: i32) -> Result<bool> {
        anime::remove_from_watchlist(&self.pool, user_id, anime_id).await.map_err(Into::into)
    }

    /// Creates a peer recommendation.
    pub async fn create_recommendation(
        &self,
        sender_id: u64,
        recipient_id: u64,
        anime_id: i32,
        note: Option<&str>,
    ) -> Result<i64> {
        anime::create_recommendation(&self.pool, sender_id, recipient_id, anime_id, note)
            .await
            .map_err(Into::into)
    }

    /// Retrieves recommendations received by a user.
    pub async fn get_received_recommendations(
        &self,
        recipient_id: u64,
        sender_filter: Option<u64>,
    ) -> Result<Vec<RecommendationDetailedEntry>> {
        anime::get_received_recommendations(&self.pool, recipient_id, sender_filter)
            .await
            .map_err(Into::into)
    }

    /// Retrieves recommendations sent by a user.
    pub async fn get_sent_recommendations(
        &self,
        sender_id: u64,
    ) -> Result<Vec<RecommendationDetailedEntry>> {
        anime::get_sent_recommendations(&self.pool, sender_id).await.map_err(Into::into)
    }

    /// Updates recommendation status (e.g. 'added').
    pub async fn mark_recommendation_status(&self, rec_id: i64, status: &str) -> Result<()> {
        anime::mark_recommendation_status(&self.pool, rec_id, status).await?;
        Ok(())
    }

    /// Retrieves an Instagram sender by numeric sender ID.
    pub async fn get_instagram_sender(&self, sender_id: &str) -> Result<Option<InstagramSender>> {
        instagram::get_sender(&self.pool, sender_id).await.map_err(Into::into)
    }

    /// Links or updates an Instagram sender ID to a username and optional Discord user.
    pub async fn upsert_instagram_sender(
        &self,
        sender_id: &str,
        username: &str,
        discord_user_id: Option<i64>,
    ) -> Result<()> {
        instagram::upsert_sender(&self.pool, sender_id, username, discord_user_id)
            .await
            .map_err(Into::into)
    }

    /// Lists all registered Instagram senders.
    pub async fn list_instagram_senders(&self) -> Result<Vec<InstagramSender>> {
        instagram::list_senders(&self.pool).await.map_err(Into::into)
    }

    /// Retrieves a bot setting by key.
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        instagram::get_setting(&self.pool, key).await.map_err(Into::into)
    }

    /// Sets a bot setting by key.
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        instagram::set_setting(&self.pool, key, value).await.map_err(Into::into)
    }

    /// Retrieves existing message IDs from a given list of snowflake IDs.
    pub async fn get_existing_message_ids(&self, message_ids: &[u64]) -> Result<HashSet<u64>> {
        messages::get_existing_message_ids(&self.pool, message_ids).await
    }

    /// Inserts a batch of new Discord messages into PostgreSQL with vector embeddings.
    pub async fn batch_insert_messages(&self, messages: &[NewDiscordMessage]) -> Result<usize> {
        messages::batch_insert_messages(&self.pool, messages).await
    }

    /// Counts total messages stored for a specific channel.
    pub async fn count_channel_messages(&self, channel_id: u64) -> Result<i64> {
        messages::count_channel_messages(&self.pool, channel_id).await
    }

    /// Counts messages stored for a channel sent by the target user.
    pub async fn count_target_user_messages(&self, channel_id: u64) -> Result<i64> {
        messages::count_target_user_messages(&self.pool, channel_id).await
    }

    /// Returns the oldest message snowflake ID stored in the database for a channel.
    pub async fn get_oldest_message_id(&self, channel_id: u64) -> Result<Option<u64>> {
        messages::get_oldest_message_id(&self.pool, channel_id).await
    }

    /// Caches movie or series metadata.
    pub async fn upsert_media_cache(&self, media: &CinemetaMedia) -> Result<()> {
        media::upsert_media_cache(&self.pool, media).await
    }

    /// Adds or updates a user's movie & series watchlist entry.
    pub async fn upsert_media_watchlist_entry(
        &self,
        user_id: u64,
        imdb_id: &str,
        status: &str,
        progress: Option<i32>,
        score: Option<i32>,
    ) -> Result<()> {
        media::upsert_media_watchlist_entry(&self.pool, user_id, imdb_id, status, progress, score).await
    }

    /// Retrieves a user's movie & series watchlist entries.
    pub async fn get_user_media_watchlist(
        &self,
        user_id: u64,
        type_filter: Option<&str>,
        status_filter: Option<&str>,
    ) -> Result<Vec<MediaWatchlistDetailedEntry>> {
        media::get_user_media_watchlist(&self.pool, user_id, type_filter, status_filter).await
    }

    /// Retrieves a unified list of watchlist entries across Anime, Movies, and TV series.
    pub async fn get_unified_watchlist(
        &self,
        user_id: u64,
        type_filter: Option<&str>,
        status_filter: Option<&str>,
    ) -> Result<Vec<UnifiedWatchlistEntry>> {
        media::get_unified_watchlist(&self.pool, user_id, type_filter, status_filter).await
    }

    /// Removes a movie or series from a user's watchlist.
    pub async fn remove_from_media_watchlist(&self, user_id: u64, imdb_id: &str) -> Result<bool> {
        media::remove_from_media_watchlist(&self.pool, user_id, imdb_id).await
    }

    /// Creates a unified peer recommendation.
    pub async fn create_unified_recommendation(
        &self,
        sender_id: u64,
        recipient_id: u64,
        media_type: &str,
        media_id: &str,
        title: &str,
        poster_url: Option<&str>,
        note: Option<&str>,
    ) -> Result<i64> {
        media::create_unified_recommendation(
            &self.pool,
            sender_id,
            recipient_id,
            media_type,
            media_id,
            title,
            poster_url,
            note,
        )
        .await
    }

    /// Retrieves received unified recommendations for a recipient.
    pub async fn get_received_unified_recommendations(
        &self,
        recipient_id: u64,
        sender_filter: Option<u64>,
    ) -> Result<Vec<UnifiedRecommendationEntry>> {
        media::get_received_unified_recommendations(&self.pool, recipient_id, sender_filter).await
    }

    /// Retrieves sent unified recommendations from a sender.
    pub async fn get_sent_unified_recommendations(
        &self,
        sender_id: u64,
    ) -> Result<Vec<UnifiedRecommendationEntry>> {
        media::get_sent_unified_recommendations(&self.pool, sender_id).await
    }

    /// Updates the status of a unified recommendation.
    pub async fn mark_unified_recommendation_status(&self, rec_id: i64, status: &str) -> Result<()> {
        media::mark_unified_recommendation_status(&self.pool, rec_id, status).await
    }
}

