use sqlx::PgPool;
use crate::anilist::AnimeMedia;
use crate::error::DbError;

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct CachedAnime {
    pub id: i32,
    pub romaji_title: String,
    pub english_title: Option<String>,
    pub cover_url: Option<String>,
    pub banner_url: Option<String>,
    pub genres: Vec<String>,
    pub episodes: Option<i32>,
    pub average_score: Option<i32>,
    pub synopsis: Option<String>,
    pub site_url: Option<String>,
}

#[allow(dead_code)]
impl CachedAnime {
    pub fn display_title(&self) -> &str {
        if let Some(ref eng) = self.english_title {
            if !eng.trim().is_empty() {
                return eng;
            }
        }
        &self.romaji_title
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct WatchlistDetailedEntry {
    pub id: i64,
    pub user_id: i64,
    pub anime_id: i32,
    pub status: String,
    pub progress: i32,
    pub score: Option<i32>,
    pub romaji_title: String,
    pub english_title: Option<String>,
    pub cover_url: Option<String>,
    pub episodes: Option<i32>,
    pub average_score: Option<i32>,
    pub site_url: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl WatchlistDetailedEntry {
    pub fn display_title(&self) -> &str {
        if let Some(ref eng) = self.english_title {
            if !eng.trim().is_empty() {
                return eng;
            }
        }
        &self.romaji_title
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct RecommendationDetailedEntry {
    pub id: i64,
    pub sender_id: i64,
    pub recipient_id: i64,
    pub anime_id: i32,
    pub note: Option<String>,
    pub status: String,
    pub romaji_title: String,
    pub english_title: Option<String>,
    pub cover_url: Option<String>,
    pub episodes: Option<i32>,
    pub average_score: Option<i32>,
    pub site_url: Option<String>,
}

impl RecommendationDetailedEntry {
    pub fn display_title(&self) -> &str {
        if let Some(ref eng) = self.english_title {
            if !eng.trim().is_empty() {
                return eng;
            }
        }
        &self.romaji_title
    }
}

/// Initializes database tables for anime cache, watchlists, and peer recommendations.
pub async fn init_tables(pool: &PgPool) -> Result<(), DbError> {
    sqlx::query(
        r#"
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
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_anime_watchlist (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL,
            anime_id INT NOT NULL REFERENCES anime_cache(id) ON DELETE CASCADE,
            status VARCHAR(20) NOT NULL DEFAULT 'planning',
            progress INT NOT NULL DEFAULT 0,
            score INT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            CONSTRAINT uq_user_anime_entry UNIQUE (user_id, anime_id)
        );
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_user_watchlist_lookup
        ON user_anime_watchlist (user_id, status);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_anime_recommendations (
            id BIGSERIAL PRIMARY KEY,
            sender_id BIGINT NOT NULL,
            recipient_id BIGINT NOT NULL,
            anime_id INT NOT NULL REFERENCES anime_cache(id) ON DELETE CASCADE,
            note TEXT,
            status VARCHAR(20) NOT NULL DEFAULT 'pending',
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_user_recommendations_recipient
        ON user_anime_recommendations (recipient_id, status);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Caches an anime's details in PostgreSQL to speed up future lookups.
pub async fn upsert_anime_cache(pool: &PgPool, anime: &AnimeMedia) -> Result<(), DbError> {
    let cover = anime.cover_image.as_ref().and_then(|c| c.best_url());
    sqlx::query(
        r#"
        INSERT INTO anime_cache (
            id, romaji_title, english_title, cover_url, banner_url,
            genres, episodes, average_score, synopsis, site_url, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW())
        ON CONFLICT (id) DO UPDATE SET
            romaji_title = EXCLUDED.romaji_title,
            english_title = EXCLUDED.english_title,
            cover_url = COALESCE(EXCLUDED.cover_url, anime_cache.cover_url),
            banner_url = COALESCE(EXCLUDED.banner_url, anime_cache.banner_url),
            genres = EXCLUDED.genres,
            episodes = EXCLUDED.episodes,
            average_score = EXCLUDED.average_score,
            synopsis = EXCLUDED.synopsis,
            site_url = EXCLUDED.site_url,
            updated_at = NOW();
        "#,
    )
    .bind(anime.id)
    .bind(&anime.title.romaji)
    .bind(&anime.title.english)
    .bind(cover)
    .bind(&anime.banner_image)
    .bind(&anime.genres)
    .bind(anime.episodes)
    .bind(anime.average_score)
    .bind(&anime.description)
    .bind(&anime.site_url)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Adds or updates an entry in the user's watchlist.
pub async fn upsert_watchlist_entry(
    pool: &PgPool,
    user_id: u64,
    anime_id: i32,
    status: &str,
    progress: Option<i32>,
    score: Option<i32>,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO user_anime_watchlist (user_id, anime_id, status, progress, score, updated_at)
        VALUES ($1, $2, $3, COALESCE($4, 0), $5, NOW())
        ON CONFLICT (user_id, anime_id) DO UPDATE SET
            status = EXCLUDED.status,
            progress = COALESCE($4, user_anime_watchlist.progress),
            score = COALESCE($5, user_anime_watchlist.score),
            updated_at = NOW();
        "#,
    )
    .bind(user_id as i64)
    .bind(anime_id)
    .bind(status)
    .bind(progress)
    .bind(score)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Retrieves a user's watchlist, optionally filtered by status ('watching', 'completed', etc.).
pub async fn get_user_watchlist(
    pool: &PgPool,
    user_id: u64,
    status_filter: Option<&str>,
) -> Result<Vec<WatchlistDetailedEntry>, DbError> {
    let entries = sqlx::query_as::<_, WatchlistDetailedEntry>(
        r#"
        SELECT 
            w.id,
            w.user_id,
            w.anime_id,
            w.status,
            w.progress,
            w.score,
            c.romaji_title,
            c.english_title,
            c.cover_url,
            c.episodes,
            c.average_score,
            c.site_url,
            w.updated_at
        FROM user_anime_watchlist w
        JOIN anime_cache c ON w.anime_id = c.id
        WHERE w.user_id = $1
          AND ($2::TEXT IS NULL OR w.status = $2)
        ORDER BY w.updated_at DESC;
        "#,
    )
    .bind(user_id as i64)
    .bind(status_filter)
    .fetch_all(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(entries)
}

/// Removes an anime entry from the user's watchlist.
pub async fn remove_from_watchlist(
    pool: &PgPool,
    user_id: u64,
    anime_id: i32,
) -> Result<bool, DbError> {
    let result = sqlx::query(
        r#"
        DELETE FROM user_anime_watchlist
        WHERE user_id = $1 AND anime_id = $2;
        "#,
    )
    .bind(user_id as i64)
    .bind(anime_id)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(result.rows_affected() > 0)
}

/// Creates a new peer recommendation from one user to another.
pub async fn create_recommendation(
    pool: &PgPool,
    sender_id: u64,
    recipient_id: u64,
    anime_id: i32,
    note: Option<&str>,
) -> Result<i64, DbError> {
    let row: (i64,) = sqlx::query_as(
        r#"
        INSERT INTO user_anime_recommendations (sender_id, recipient_id, anime_id, note, status, updated_at)
        VALUES ($1, $2, $3, $4, 'pending', NOW())
        RETURNING id;
        "#,
    )
    .bind(sender_id as i64)
    .bind(recipient_id as i64)
    .bind(anime_id)
    .bind(note)
    .fetch_one(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.0)
}

/// Retrieves recommendations received by a user.
pub async fn get_received_recommendations(
    pool: &PgPool,
    recipient_id: u64,
    sender_filter: Option<u64>,
) -> Result<Vec<RecommendationDetailedEntry>, DbError> {
    let entries = sqlx::query_as::<_, RecommendationDetailedEntry>(
        r#"
        SELECT 
            r.id,
            r.sender_id,
            r.recipient_id,
            r.anime_id,
            r.note,
            r.status,
            c.romaji_title,
            c.english_title,
            c.cover_url,
            c.episodes,
            c.average_score,
            c.site_url
        FROM user_anime_recommendations r
        JOIN anime_cache c ON r.anime_id = c.id
        WHERE r.recipient_id = $1
          AND ($2::BIGINT IS NULL OR r.sender_id = $2)
        ORDER BY r.created_at DESC;
        "#,
    )
    .bind(recipient_id as i64)
    .bind(sender_filter.map(|s| s as i64))
    .fetch_all(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(entries)
}

/// Retrieves recommendations sent by a user.
pub async fn get_sent_recommendations(
    pool: &PgPool,
    sender_id: u64,
) -> Result<Vec<RecommendationDetailedEntry>, DbError> {
    let entries = sqlx::query_as::<_, RecommendationDetailedEntry>(
        r#"
        SELECT 
            r.id,
            r.sender_id,
            r.recipient_id,
            r.anime_id,
            r.note,
            r.status,
            c.romaji_title,
            c.english_title,
            c.cover_url,
            c.episodes,
            c.average_score,
            c.site_url
        FROM user_anime_recommendations r
        JOIN anime_cache c ON r.anime_id = c.id
        WHERE r.sender_id = $1
        ORDER BY r.created_at DESC;
        "#,
    )
    .bind(sender_id as i64)
    .fetch_all(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(entries)
}

/// Updates the status of a recommendation (e.g. marked as 'added').
pub async fn mark_recommendation_status(
    pool: &PgPool,
    rec_id: i64,
    status: &str,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        UPDATE user_anime_recommendations
        SET status = $1, updated_at = NOW()
        WHERE id = $2;
        "#,
    )
    .bind(status)
    .bind(rec_id)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anilist::{AnimeCoverImage, AnimeTitle};
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    async fn test_anime_db_lifecycle() {
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/discord_ai".to_string());

        let pool = match PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Skipping anime db test (db unreachable at {db_url}): {e}");
                return;
            }
        };

        init_tables(&pool).await.expect("init_tables should succeed");

        let test_anime = AnimeMedia {
            id: 99999,
            title: AnimeTitle {
                romaji: "Test Anime Romaji".to_string(),
                english: Some("Test Anime English".to_string()),
            },
            format: Some("TV".to_string()),
            status: Some("FINISHED".to_string()),
            episodes: Some(12),
            season_year: Some(2024),
            genres: vec!["Sci-Fi".to_string(), "Action".to_string()],
            average_score: Some(88),
            description: Some("Test synopsis".to_string()),
            site_url: Some("https://anilist.co/anime/99999".to_string()),
            cover_image: Some(AnimeCoverImage {
                extra_large: None,
                large: Some("https://example.com/cover.jpg".to_string()),
                medium: None,
            }),
            banner_image: None,
        };

        // 1. Cache anime
        upsert_anime_cache(&pool, &test_anime)
            .await
            .expect("upsert_anime_cache should succeed");

        // 2. Add to user watchlist
        let user_id = 111222333;
        upsert_watchlist_entry(&pool, user_id, 99999, "watching", Some(3), Some(9))
            .await
            .expect("upsert_watchlist_entry should succeed");

        let list = get_user_watchlist(&pool, user_id, Some("watching"))
            .await
            .expect("get_user_watchlist should succeed");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].display_title(), "Test Anime English");
        assert_eq!(list[0].progress, 3);
        assert_eq!(list[0].score, Some(9));

        // 3. Recommendation
        let recipient_id = 444555666;
        let rec_id = create_recommendation(
            &pool,
            user_id,
            recipient_id,
            99999,
            Some("Check this out!"),
        )
        .await
        .expect("create_recommendation should succeed");
        assert!(rec_id > 0);

        let recs = get_received_recommendations(&pool, recipient_id, None)
            .await
            .expect("get_received_recommendations should succeed");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].status, "pending");
        assert_eq!(recs[0].note.as_deref(), Some("Check this out!"));

        // 4. Mark status
        mark_recommendation_status(&pool, rec_id, "added")
            .await
            .expect("mark_recommendation_status should succeed");

        // 5. Remove from watchlist
        let removed = remove_from_watchlist(&pool, user_id, 99999)
            .await
            .expect("remove_from_watchlist should succeed");
        assert!(removed);
    }
}
