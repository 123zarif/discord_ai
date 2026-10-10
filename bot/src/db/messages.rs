use std::collections::HashSet;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use crate::error::{DbError, Result};

#[derive(Debug, Clone)]
pub struct NewDiscordMessage {
    pub message_id: u64,
    pub channel_id: u64,
    pub guild_id: u64,
    pub author_id: u64,
    pub author_name: String,
    pub content: String,
    pub is_target_user: bool,
    pub reply_to_message_id: Option<u64>,
    pub message_timestamp: DateTime<Utc>,
    pub embedding: Option<Vec<f32>>,
}

/// Initializes database tables and indexes for Discord messages and vector embeddings.
pub async fn init_tables(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
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
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_discord_messages_channel
        ON discord_messages (channel_id, message_timestamp DESC);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_discord_messages_author
        ON discord_messages (author_id);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_discord_messages_target_user
        ON discord_messages (is_target_user);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_discord_messages_embedding_hnsw
        ON discord_messages USING hnsw (embedding vector_cosine_ops);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Returns the subset of message IDs that already exist in PostgreSQL.
pub async fn get_existing_message_ids(
    pool: &PgPool,
    message_ids: &[u64],
) -> Result<HashSet<u64>> {
    if message_ids.is_empty() {
        return Ok(HashSet::new());
    }

    let raw_ids: Vec<i64> = message_ids.iter().map(|&id| id as i64).collect();

    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT message_id FROM discord_messages WHERE message_id = ANY($1)",
    )
    .bind(&raw_ids)
    .fetch_all(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(rows.into_iter().map(|(id,)| id as u64).collect())
}

/// Inserts a batch of new Discord messages. Skips messages that already exist (idempotent).
pub async fn batch_insert_messages(
    pool: &PgPool,
    messages: &[NewDiscordMessage],
) -> Result<usize> {
    if messages.is_empty() {
        return Ok(0);
    }

    let mut tx = pool.begin().await.map_err(DbError::QueryFailed)?;
    let mut inserted_count = 0;

    for msg in messages {
        let pg_vector_str = msg.embedding.as_ref().map(|vec| {
            format!(
                "[{}]",
                vec.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        });

        let rows_affected = sqlx::query(
            r#"
            INSERT INTO discord_messages (
                message_id,
                channel_id,
                guild_id,
                author_id,
                author_name,
                content,
                is_target_user,
                reply_to_message_id,
                message_timestamp,
                embedding
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::vector)
            ON CONFLICT (message_id) DO NOTHING
            "#,
        )
        .bind(msg.message_id as i64)
        .bind(msg.channel_id as i64)
        .bind(msg.guild_id as i64)
        .bind(msg.author_id as i64)
        .bind(&msg.author_name)
        .bind(&msg.content)
        .bind(msg.is_target_user)
        .bind(msg.reply_to_message_id.map(|id| id as i64))
        .bind(msg.message_timestamp)
        .bind(pg_vector_str)
        .execute(&mut *tx)
        .await
        .map_err(DbError::QueryFailed)?
        .rows_affected();

        inserted_count += rows_affected as usize;
    }

    tx.commit().await.map_err(DbError::QueryFailed)?;
    Ok(inserted_count)
}

/// Counts total messages stored for a specific channel.
pub async fn count_channel_messages(pool: &PgPool, channel_id: u64) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM discord_messages WHERE channel_id = $1",
    )
    .bind(channel_id as i64)
    .fetch_one(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.0)
}

/// Counts target user messages stored for a specific channel.
pub async fn count_target_user_messages(pool: &PgPool, channel_id: u64) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM discord_messages WHERE channel_id = $1 AND is_target_user = true",
    )
    .bind(channel_id as i64)
    .fetch_one(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.0)
}

/// Returns the oldest message snowflake ID stored in the database for a given channel.
pub async fn get_oldest_message_id(pool: &PgPool, channel_id: u64) -> Result<Option<u64>> {
    let row: (Option<i64>,) = sqlx::query_as(
        "SELECT MIN(message_id) FROM discord_messages WHERE channel_id = $1",
    )
    .bind(channel_id as i64)
    .fetch_one(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.0.map(|id| id as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    async fn test_messages_db_lifecycle() {
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
                eprintln!("Skipping messages database test (db unreachable at {db_url}): {e}");
                return;
            }
        };

        // Initialize table
        init_tables(&pool).await.expect("init_tables should succeed");

        let test_channel_id = 9988776655u64;
        let test_msg_id_1 = 1122334455u64;
        let test_msg_id_2 = 1122334456u64;

        // Cleanup before test
        let _ = sqlx::query("DELETE FROM discord_messages WHERE channel_id = $1")
            .bind(test_channel_id as i64)
            .execute(&pool)
            .await;

        let dummy_embedding = vec![0.05f32; 384];

        let msg1 = NewDiscordMessage {
            message_id: test_msg_id_1,
            channel_id: test_channel_id,
            guild_id: 12345,
            author_id: 111,
            author_name: "User1".to_string(),
            content: "Hello from test 1".to_string(),
            is_target_user: false,
            reply_to_message_id: None,
            message_timestamp: Utc::now(),
            embedding: Some(dummy_embedding.clone()),
        };

        let msg2 = NewDiscordMessage {
            message_id: test_msg_id_2,
            channel_id: test_channel_id,
            guild_id: 12345,
            author_id: 222,
            author_name: "TargetUser".to_string(),
            content: "Hello from target user".to_string(),
            is_target_user: true,
            reply_to_message_id: Some(test_msg_id_1),
            message_timestamp: Utc::now(),
            embedding: Some(dummy_embedding),
        };

        let inserted = batch_insert_messages(&pool, &[msg1, msg2])
            .await
            .expect("Batch insert should succeed");
        assert_eq!(inserted, 2);

        // Test existing IDs filter
        let existing = get_existing_message_ids(&pool, &[test_msg_id_1, test_msg_id_2, 9999999999])
            .await
            .expect("get_existing_message_ids should succeed");
        assert!(existing.contains(&test_msg_id_1));
        assert!(existing.contains(&test_msg_id_2));
        assert!(!existing.contains(&9999999999));

        // Test idempotency: re-inserting existing messages should insert 0
        let re_msg = NewDiscordMessage {
            message_id: test_msg_id_1,
            channel_id: test_channel_id,
            guild_id: 12345,
            author_id: 111,
            author_name: "User1".to_string(),
            content: "Duplicate attempt".to_string(),
            is_target_user: false,
            reply_to_message_id: None,
            message_timestamp: Utc::now(),
            embedding: None,
        };
        let re_inserted = batch_insert_messages(&pool, &[re_msg])
            .await
            .expect("Re-insert should succeed");
        assert_eq!(re_inserted, 0);

        // Test counts
        let total = count_channel_messages(&pool, test_channel_id)
            .await
            .expect("count_channel_messages should succeed");
        assert_eq!(total, 2);

        let target_total = count_target_user_messages(&pool, test_channel_id)
            .await
            .expect("count_target_user_messages should succeed");
        assert_eq!(target_total, 1);

        // Test oldest message ID retrieval
        let oldest = get_oldest_message_id(&pool, test_channel_id)
            .await
            .expect("get_oldest_message_id should succeed");
        assert_eq!(oldest, Some(test_msg_id_1));

        // Clean up test channel messages
        let _ = sqlx::query("DELETE FROM discord_messages WHERE channel_id = $1")
            .bind(test_channel_id as i64)
            .execute(&pool)
            .await;
    }
}

