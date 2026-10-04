use sqlx::PgPool;
use crate::error::DbError;

/// Initializes database tables required for user action tracking.
pub async fn init_tables(pool: &PgPool) -> Result<(), DbError> {
    sqlx::query(
        r#"
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
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_action_counts_lookup 
        ON action_counts (guild_id, user_id, target_id, action_type);
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    sqlx::query(
        r#"
        -- Merge any legacy reverse rows into canonical (min, max) pairs
        DO $$
        BEGIN
            UPDATE action_counts ac1
            SET count = ac1.count + ac2.count,
                updated_at = GREATEST(ac1.updated_at, ac2.updated_at)
            FROM action_counts ac2
            WHERE ac1.guild_id = ac2.guild_id
              AND ac1.action_type = ac2.action_type
              AND ac1.user_id = ac2.target_id
              AND ac1.target_id = ac2.user_id
              AND ac1.user_id < ac1.target_id;

            DELETE FROM action_counts ac2
            USING action_counts ac1
            WHERE ac1.guild_id = ac2.guild_id
              AND ac1.action_type = ac2.action_type
              AND ac1.user_id = ac2.target_id
              AND ac1.target_id = ac2.user_id
              AND ac2.user_id > ac2.target_id;

            UPDATE action_counts
            SET user_id = target_id, target_id = user_id
            WHERE user_id > target_id;
        END $$;
        "#,
    )
    .execute(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(())
}

/// Canonicalizes user and target IDs into a consistent ordered pair (min, max).
/// This ensures interactions between two users (e.g. A kisses B, B kisses A)
/// share the exact same interaction counter.
#[inline]
pub fn canonicalize_pair(user_id: u64, target_id: u64) -> (i64, i64) {
    if user_id <= target_id {
        (user_id as i64, target_id as i64)
    } else {
        (target_id as i64, user_id as i64)
    }
}

/// Atomically increments the interaction counter between user and target for a given action type.
/// Returns the new total count.
pub async fn increment_action_count(
    pool: &PgPool,
    guild_id: u64,
    user_id: u64,
    target_id: u64,
    action_type: &str,
) -> Result<i64, DbError> {
    let (u1, u2) = canonicalize_pair(user_id, target_id);
    let row: (i64,) = sqlx::query_as(
        r#"
        INSERT INTO action_counts (guild_id, user_id, target_id, action_type, count, updated_at)
        VALUES ($1, $2, $3, $4, 1, NOW())
        ON CONFLICT (guild_id, user_id, target_id, action_type)
        DO UPDATE SET
            count = action_counts.count + 1,
            updated_at = NOW()
        RETURNING count;
        "#,
    )
    .bind(guild_id as i64)
    .bind(u1)
    .bind(u2)
    .bind(action_type)
    .fetch_one(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.0)
}

/// Atomically decrements the interaction counter between user and target for a given action type
/// (down to a minimum of 0). Returns the updated count. If no entry existed, returns 0.
pub async fn decrement_action_count(
    pool: &PgPool,
    guild_id: u64,
    user_id: u64,
    target_id: u64,
    action_type: &str,
) -> Result<i64, DbError> {
    let (u1, u2) = canonicalize_pair(user_id, target_id);
    let row: Option<(i64,)> = sqlx::query_as(
        r#"
        UPDATE action_counts
        SET count = GREATEST(0, count - 1),
            updated_at = NOW()
        WHERE guild_id = $1 AND user_id = $2 AND target_id = $3 AND action_type = $4
        RETURNING count;
        "#,
    )
    .bind(guild_id as i64)
    .bind(u1)
    .bind(u2)
    .bind(action_type)
    .fetch_optional(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.map(|r| r.0).unwrap_or(0))
}

/// Retrieves the current interaction count between user and target for a given action type.
pub async fn get_action_count(
    pool: &PgPool,
    guild_id: u64,
    user_id: u64,
    target_id: u64,
    action_type: &str,
) -> Result<i64, DbError> {
    let (u1, u2) = canonicalize_pair(user_id, target_id);
    let row: Option<(i64,)> = sqlx::query_as(
        r#"
        SELECT count FROM action_counts
        WHERE guild_id = $1 AND user_id = $2 AND target_id = $3 AND action_type = $4;
        "#,
    )
    .bind(guild_id as i64)
    .bind(u1)
    .bind(u2)
    .bind(action_type)
    .fetch_optional(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    Ok(row.map(|r| r.0).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    async fn test_action_counts_lifecycle() {
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/discord_ai".to_string());

        let pool = match PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Skipping action counts database test (db unreachable at {db_url}): {e}");
                return;
            }
        };

        // Initialize table
        init_tables(&pool).await.expect("init_tables should succeed");

        let test_user = 999111;
        let test_target = 888222;
        let action = "test_kiss";

        let count1 = increment_action_count(&pool, 0, test_user, test_target, action)
            .await
            .expect("First increment should succeed");
        assert!(count1 >= 1);

        let count2 = increment_action_count(&pool, 0, test_user, test_target, action)
            .await
            .expect("Second increment should succeed");
        assert_eq!(count2, count1 + 1);

        let fetched = get_action_count(&pool, 0, test_user, test_target, action)
            .await
            .expect("get_action_count should succeed");
        assert_eq!(fetched, count2);

        let decremented = decrement_action_count(&pool, 0, test_user, test_target, action)
            .await
            .expect("decrement_action_count should succeed");
        assert_eq!(decremented, count2 - 1);

        // Test mutual/reversed pair increments the same counter
        let count_reversed = increment_action_count(&pool, 0, test_target, test_user, action)
            .await
            .expect("Reversed pair increment should succeed");
        assert_eq!(count_reversed, count2);
    }
}
