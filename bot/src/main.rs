pub mod anilist;
mod config;
mod db;
mod discord;
mod error;
pub mod instagram;

use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::Config;
use crate::db::Database;
use crate::error::{DbError, Result};

#[tokio::main]
async fn main() {
    let default_filter = "info";
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting bot...");

    if let Err(err) = run_app().await {
        error!("Application error: {err}");
        std::process::exit(1);
    }
}

async fn run_app() -> Result<()> {
    info!("Loading configuration...");
    let config = Config::from_env()?;
    let config = Arc::new(config);

    info!("Connecting to PostgreSQL...");
    let db = Database::connect(&config.database_url).await?;
    info!("PostgreSQL connected");

    let health = db.health_check().await?;
    if !health.pgvector_available {
        return Err(DbError::PgVectorCheckFailed(
            "pgvector extension is not installed in the target database".to_string(),
        )
        .into());
    }
    info!("pgvector available");

    db.init_tables().await?;
    info!("Database tables initialized");

    info!("Connecting to Discord...");
    discord::run(config, db).await?;

    Ok(())
}
