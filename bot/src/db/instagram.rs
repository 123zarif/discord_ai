use sqlx::PgPool;
use crate::error::DbError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstagramSender {
    pub sender_id: String,
    pub username: String,
    pub discord_user_id: Option<i64>,
}

/// Initializes database tables for Instagram reel integration.
pub async fn init_tables(pool: &PgPool) -> Result<(), DbError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS instagram_senders (
            sender_id VARCHAR(64) PRIMARY KEY,
            username VARCHAR(100) NOT NULL,
            discord_user_id BIGINT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bot_settings (
            key VARCHAR(64) PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    // Seed initial known senders from old bot
    sqlx::query(
        r#"
        INSERT INTO instagram_senders (sender_id, username)
        VALUES 
            ('1177907484531522', 'Zarif_1020'),
            ('999126642939413', 'srijan.robi'),
            ('3343658449150786', 'simak.ahamed')
        ON CONFLICT (sender_id) DO NOTHING;
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Retrieves an Instagram sender by numeric sender ID.
pub async fn get_sender(pool: &PgPool, sender_id: &str) -> Result<Option<InstagramSender>, DbError> {
    sqlx::query_as::<_, InstagramSender>(
        r#"
        SELECT sender_id, username, discord_user_id
        FROM instagram_senders
        WHERE sender_id = $1
        "#,
    )
    .bind(sender_id)
    .fetch_optional(pool)
    .await
    .map_err(DbError::QueryFailed)
}

/// Links or updates an Instagram sender ID to a username and optional Discord user.
pub async fn upsert_sender(
    pool: &PgPool,
    sender_id: &str,
    username: &str,
    discord_user_id: Option<i64>,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO instagram_senders (sender_id, username, discord_user_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (sender_id) DO UPDATE SET
            username = EXCLUDED.username,
            discord_user_id = EXCLUDED.discord_user_id
        "#,
    )
    .bind(sender_id)
    .bind(username)
    .bind(discord_user_id)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Lists all registered Instagram senders.
pub async fn list_senders(pool: &PgPool) -> Result<Vec<InstagramSender>, DbError> {
    sqlx::query_as::<_, InstagramSender>(
        r#"
        SELECT sender_id, username, discord_user_id
        FROM instagram_senders
        ORDER BY username ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(DbError::QueryFailed)
}

/// Retrieves a persistent bot setting by key.
pub async fn get_setting(pool: &PgPool, key: &str) -> Result<Option<String>, DbError> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT value FROM bot_settings WHERE key = $1
        "#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.map(|r| r.0))
}

/// Upserts a persistent bot setting by key.
pub async fn set_setting(pool: &PgPool, key: &str, value: &str) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO bot_settings (key, value, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (key) DO UPDATE SET
            value = EXCLUDED.value,
            updated_at = NOW()
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}
