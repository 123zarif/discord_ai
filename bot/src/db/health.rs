use std::time::Instant;
use sqlx::PgPool;
use crate::error::DbError;

#[derive(Debug, Clone)]
pub struct DatabaseHealthReport {
    pub connected: bool,
    pub latency_ms: Option<u64>,
    pub pgvector_available: bool,
    pub pgvector_version: Option<String>,
    pub pool_size: u32,
    pub idle_connections: u32,
}

/// Executes live connectivity and pgvector extension verification against PostgreSQL.
pub async fn check_health(pool: &PgPool) -> Result<DatabaseHealthReport, DbError> {
    let start = Instant::now();

    // 1. Verify live database connectivity
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map_err(DbError::ConnectionFailed)?;
    let latency_ms = start.elapsed().as_millis() as u64;

    // 2. Query pg_extension to verify that pgvector is installed
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT extversion FROM pg_extension WHERE extname = 'vector'",
    )
    .fetch_optional(pool)
    .await
    .map_err(DbError::QueryFailed)?;

    let (pgvector_available, pgvector_version) = match row {
        Some((version,)) => (true, Some(version)),
        None => (false, None),
    };

    Ok(DatabaseHealthReport {
        connected: true,
        latency_ms: Some(latency_ms),
        pgvector_available,
        pgvector_version,
        pool_size: pool.size(),
        idle_connections: pool.num_idle() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    async fn test_live_database_health_and_pgvector() {
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/discord_ai".to_string());

        let pool = match PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Skipping live database test (database unreachable at {db_url}): {e}");
                return;
            }
        };

        let health = check_health(&pool)
            .await
            .expect("Health check should succeed against running database");

        assert!(health.connected, "Database should be connected");
        assert!(health.pgvector_available, "pgvector extension must be detected");
        assert!(health.pgvector_version.is_some(), "pgvector version should not be None");
    }
}
