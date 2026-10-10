use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use crate::cinemeta::CinemetaMedia;
use crate::error::{DbError, Result};

#[derive(Debug, Clone, FromRow)]
pub struct MediaWatchlistDetailedEntry {
    pub id: i64,
    pub user_id: i64,
    pub imdb_id: String,
    pub media_type: String,
    pub title: String,
    pub year: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub genres: Vec<String>,
    pub imdb_rating: Option<String>,
    pub runtime: Option<String>,
    pub synopsis: Option<String>,
    pub status: String,
    pub progress: i32,
    pub score: Option<i32>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UnifiedWatchlistEntry {
    pub media_type: String, // "anime", "movie", "series"
    pub media_id: String,   // AniList ID or IMDb ID
    pub title: String,
    pub year: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub status: String,
    pub progress: i32,
    pub total_progress: Option<i32>,
    pub score: Option<i32>,
    pub imdb_rating: Option<String>,
    pub site_url: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct UnifiedRecommendationEntry {
    pub id: i64,
    pub sender_id: i64,
    pub recipient_id: i64,
    pub media_type: String,
    pub media_id: String,
    pub title: String,
    pub poster_url: Option<String>,
    pub note: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// Initializes database tables and indexes for movies, series, and unified recommendations.
pub async fn init_tables(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS media_cache (
            imdb_id VARCHAR(32) PRIMARY KEY,
            media_type VARCHAR(16) NOT NULL,
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
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_media_watchlist (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL,
            imdb_id VARCHAR(32) NOT NULL REFERENCES media_cache(imdb_id) ON DELETE CASCADE,
            status VARCHAR(20) NOT NULL DEFAULT 'planning',
            progress INT NOT NULL DEFAULT 0,
            score INT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            CONSTRAINT uq_user_media_entry UNIQUE (user_id, imdb_id)
        );
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_user_media_watchlist_lookup
        ON user_media_watchlist (user_id, status);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_unified_recommendations (
            id BIGSERIAL PRIMARY KEY,
            sender_id BIGINT NOT NULL,
            recipient_id BIGINT NOT NULL,
            media_type VARCHAR(16) NOT NULL,
            media_id VARCHAR(64) NOT NULL,
            title TEXT NOT NULL,
            poster_url TEXT,
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
        CREATE INDEX IF NOT EXISTS idx_user_unified_recommendations_recipient
        ON user_unified_recommendations (recipient_id, status);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Caches movie or TV series metadata from Cinemeta.
pub async fn upsert_media_cache(pool: &PgPool, media: &CinemetaMedia) -> Result<()> {
    let imdb_id = media.id();
    let year = media.display_year();

    sqlx::query(
        r#"
        INSERT INTO media_cache (
            imdb_id, media_type, title, year, poster_url, backdrop_url,
            genres, imdb_rating, runtime, synopsis, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW())
        ON CONFLICT (imdb_id) DO UPDATE SET
            media_type = EXCLUDED.media_type,
            title = EXCLUDED.title,
            year = COALESCE(EXCLUDED.year, media_cache.year),
            poster_url = COALESCE(EXCLUDED.poster_url, media_cache.poster_url),
            backdrop_url = COALESCE(EXCLUDED.backdrop_url, media_cache.backdrop_url),
            genres = EXCLUDED.genres,
            imdb_rating = COALESCE(EXCLUDED.imdb_rating, media_cache.imdb_rating),
            runtime = COALESCE(EXCLUDED.runtime, media_cache.runtime),
            synopsis = COALESCE(EXCLUDED.synopsis, media_cache.synopsis),
            updated_at = NOW()
        "#,
    )
    .bind(imdb_id)
    .bind(&media.media_type)
    .bind(&media.name)
    .bind(year)
    .bind(&media.poster)
    .bind(&media.background)
    .bind(&media.genres)
    .bind(&media.imdb_rating)
    .bind(&media.runtime)
    .bind(&media.description)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Adds or updates an entry in the user's movie & series watchlist.
pub async fn upsert_media_watchlist_entry(
    pool: &PgPool,
    user_id: u64,
    imdb_id: &str,
    status: &str,
    progress: Option<i32>,
    score: Option<i32>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO user_media_watchlist (user_id, imdb_id, status, progress, score, updated_at)
        VALUES ($1, $2, $3, COALESCE($4, 0), $5, NOW())
        ON CONFLICT (user_id, imdb_id) DO UPDATE SET
            status = EXCLUDED.status,
            progress = CASE WHEN $4 IS NOT NULL THEN EXCLUDED.progress ELSE user_media_watchlist.progress END,
            score = CASE WHEN $5 IS NOT NULL THEN EXCLUDED.score ELSE user_media_watchlist.score END,
            updated_at = NOW()
        "#,
    )
    .bind(user_id as i64)
    .bind(imdb_id)
    .bind(status)
    .bind(progress)
    .bind(score)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Fetches a user's movie & TV series watchlist entries with joined metadata.
pub async fn get_user_media_watchlist(
    pool: &PgPool,
    user_id: u64,
    type_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<MediaWatchlistDetailedEntry>> {
    let mut query_str = String::from(
        r#"
        SELECT
            w.id,
            w.user_id,
            w.imdb_id,
            m.media_type,
            m.title,
            m.year,
            m.poster_url,
            m.backdrop_url,
            m.genres,
            m.imdb_rating,
            m.runtime,
            m.synopsis,
            w.status,
            w.progress,
            w.score,
            w.updated_at
        FROM user_media_watchlist w
        JOIN media_cache m ON w.imdb_id = m.imdb_id
        WHERE w.user_id = $1
        "#,
    );

    if type_filter.is_some() {
        query_str.push_str(" AND m.media_type = $2");
    }
    if status_filter.is_some() {
        if type_filter.is_some() {
            query_str.push_str(" AND w.status = $3");
        } else {
            query_str.push_str(" AND w.status = $2");
        }
    }

    query_str.push_str(" ORDER BY w.updated_at DESC");

    let mut q = sqlx::query_as::<_, MediaWatchlistDetailedEntry>(&query_str).bind(user_id as i64);

    if let Some(tf) = type_filter {
        q = q.bind(tf);
    }
    if let Some(sf) = status_filter {
        q = q.bind(sf);
    }

    let entries = q.fetch_all(pool).await.map_err(DbError::QueryFailed)?;
    Ok(entries)
}

/// Retrieves a unified list of watchlist entries across Anime, Movies, and TV series.
pub async fn get_unified_watchlist(
    pool: &PgPool,
    user_id: u64,
    type_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<UnifiedWatchlistEntry>> {
    let mut entries = Vec::new();

    let fetch_anime = match type_filter {
        None | Some("all") | Some("anime") => true,
        _ => false,
    };

    let fetch_media = match type_filter {
        None | Some("all") | Some("movie") | Some("series") => true,
        _ => false,
    };

    if fetch_anime {
        let anime_entries = crate::db::anime::get_user_watchlist(pool, user_id, status_filter).await?;
        for a in anime_entries {
            let site = a.site_url.clone().unwrap_or_else(|| format!("https://anilist.co/anime/{}", a.anime_id));
            entries.push(UnifiedWatchlistEntry {
                media_type: "anime".to_string(),
                media_id: a.anime_id.to_string(),
                title: a.display_title().to_string(),
                year: None,
                poster_url: a.cover_url,
                backdrop_url: None,
                status: a.status,
                progress: a.progress,
                total_progress: a.episodes,
                score: a.score,
                imdb_rating: None,
                site_url: site,
                updated_at: a.updated_at,
            });
        }
    }

    if fetch_media {
        let media_type_arg = match type_filter {
            Some("movie") => Some("movie"),
            Some("series") => Some("series"),
            _ => None,
        };
        let media_entries = get_user_media_watchlist(pool, user_id, media_type_arg, status_filter).await?;
        for m in media_entries {
            let site = format!("https://www.imdb.com/title/{}/", m.imdb_id);
            entries.push(UnifiedWatchlistEntry {
                media_type: m.media_type,
                media_id: m.imdb_id,
                title: m.title,
                year: m.year,
                poster_url: m.poster_url,
                backdrop_url: m.backdrop_url,
                status: m.status,
                progress: m.progress,
                total_progress: None,
                score: m.score,
                imdb_rating: m.imdb_rating,
                site_url: site,
                updated_at: m.updated_at,
            });
        }
    }

    // Sort combined entries by updated_at descending
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    Ok(entries)
}

/// Removes a movie or series from a user's watchlist.
pub async fn remove_from_media_watchlist(
    pool: &PgPool,
    user_id: u64,
    imdb_id: &str,
) -> Result<bool> {
    let res = sqlx::query(
        "DELETE FROM user_media_watchlist WHERE user_id = $1 AND imdb_id = $2",
    )
    .bind(user_id as i64)
    .bind(imdb_id)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(res.rows_affected() > 0)
}

/// Creates a unified peer recommendation.
pub async fn create_unified_recommendation(
    pool: &PgPool,
    sender_id: u64,
    recipient_id: u64,
    media_type: &str,
    media_id: &str,
    title: &str,
    poster_url: Option<&str>,
    note: Option<&str>,
) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        r#"
        INSERT INTO user_unified_recommendations (
            sender_id, recipient_id, media_type, media_id, title, poster_url, note, status, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', NOW(), NOW())
        RETURNING id
        "#,
    )
    .bind(sender_id as i64)
    .bind(recipient_id as i64)
    .bind(media_type)
    .bind(media_id)
    .bind(title)
    .bind(poster_url)
    .bind(note)
    .fetch_one(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.0)
}

/// Retrieves received unified recommendations for a recipient.
pub async fn get_received_unified_recommendations(
    pool: &PgPool,
    recipient_id: u64,
    sender_filter: Option<u64>,
) -> Result<Vec<UnifiedRecommendationEntry>> {
    let mut query_str = String::from(
        r#"
        SELECT id, sender_id, recipient_id, media_type, media_id, title, poster_url, note, status, created_at
        FROM user_unified_recommendations
        WHERE recipient_id = $1
        "#,
    );

    if sender_filter.is_some() {
        query_str.push_str(" AND sender_id = $2");
    }

    query_str.push_str(" ORDER BY created_at DESC LIMIT 25");

    let mut q = sqlx::query_as::<_, UnifiedRecommendationEntry>(&query_str).bind(recipient_id as i64);

    if let Some(sender) = sender_filter {
        q = q.bind(sender as i64);
    }

    let entries = q.fetch_all(pool).await.map_err(DbError::QueryFailed)?;
    Ok(entries)
}

/// Retrieves sent unified recommendations from a sender.
pub async fn get_sent_unified_recommendations(
    pool: &PgPool,
    sender_id: u64,
) -> Result<Vec<UnifiedRecommendationEntry>> {
    let entries = sqlx::query_as::<_, UnifiedRecommendationEntry>(
        r#"
        SELECT id, sender_id, recipient_id, media_type, media_id, title, poster_url, note, status, created_at
        FROM user_unified_recommendations
        WHERE sender_id = $1
        ORDER BY created_at DESC
        LIMIT 25
        "#,
    )
    .bind(sender_id as i64)
    .fetch_all(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(entries)
}

/// Updates the status of a unified recommendation.
pub async fn mark_unified_recommendation_status(
    pool: &PgPool,
    rec_id: i64,
    status: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE user_unified_recommendations SET status = $1, updated_at = NOW() WHERE id = $2",
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
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    async fn test_media_db_lifecycle() {
        let _ = dotenvy::dotenv();
        let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/discord_ai".to_string()
        });

        let pool = match PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Skipping media database test (db unreachable at {db_url}): {e}");
                return;
            }
        };

        init_tables(&pool).await.expect("init_tables should succeed");

        let test_media = CinemetaMedia {
            imdb_id: Some("tt1375666_test".to_string()),
            media_type: "movie".to_string(),
            name: "Test Inception".to_string(),
            year: Some("2010".to_string()),
            released: Some("2010-07-16".to_string()),
            poster: Some("https://example.com/poster.jpg".to_string()),
            background: Some("https://example.com/backdrop.jpg".to_string()),
            genres: vec!["Sci-Fi".to_string(), "Action".to_string()],
            imdb_rating: Some("8.8".to_string()),
            runtime: Some("148 min".to_string()),
            description: Some("A dream within a dream.".to_string()),
            status: None,
        };

        upsert_media_cache(&pool, &test_media)
            .await
            .expect("upsert_media_cache should succeed");

        let test_user = 99881122u64;

        upsert_media_watchlist_entry(
            &pool,
            test_user,
            "tt1375666_test",
            "watching",
            Some(45),
            Some(9),
        )
        .await
        .expect("upsert_media_watchlist_entry should succeed");

        let list = get_user_media_watchlist(&pool, test_user, Some("movie"), None)
            .await
            .expect("get_user_media_watchlist should succeed");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Test Inception");
        assert_eq!(list[0].score, Some(9));

        let rec_id = create_unified_recommendation(
            &pool,
            test_user,
            1234567,
            "movie",
            "tt1375666_test",
            "Test Inception",
            Some("https://example.com/poster.jpg"),
            Some("Must watch!"),
        )
        .await
        .expect("create_unified_recommendation should succeed");

        let received = get_received_unified_recommendations(&pool, 1234567, None)
            .await
            .expect("get_received_unified_recommendations should succeed");
        assert!(received.iter().any(|r| r.id == rec_id));

        mark_unified_recommendation_status(&pool, rec_id, "added")
            .await
            .expect("mark_unified_recommendation_status should succeed");

        let removed = remove_from_media_watchlist(&pool, test_user, "tt1375666_test")
            .await
            .expect("remove_from_media_watchlist should succeed");
        assert!(removed);

        let _ = sqlx::query("DELETE FROM user_unified_recommendations WHERE id = $1")
            .bind(rec_id)
            .execute(&pool)
            .await;
        let _ = sqlx::query("DELETE FROM media_cache WHERE imdb_id = 'tt1375666_test'")
            .execute(&pool)
            .await;
    }
}

