use thiserror::Error;

pub type Result<T, E = AppError> = std::result::Result<T, E>;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Database error: {0}")]
    Database(#[from] DbError),

    #[error("Discord error: {0}")]
    Discord(#[from] serenity::Error),

    #[allow(dead_code)]
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    MissingVariable(String),

    #[error("Invalid value for environment variable '{name}': {reason}")]
    InvalidValue { name: String, reason: String },
}

#[derive(Error, Debug)]
pub enum DbError {
    #[error("Failed to connect to PostgreSQL: {0}")]
    ConnectionFailed(#[source] sqlx::Error),

    #[error("Failed to execute database query: {0}")]
    QueryFailed(#[source] sqlx::Error),

    #[error("pgvector extension check failed: {0}")]
    PgVectorCheckFailed(String),
}
