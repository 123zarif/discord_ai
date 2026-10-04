pub mod actions;
pub mod client;
pub mod commands;
pub mod events;
pub mod permissions;

use std::sync::Arc;
use crate::config::Config;
use crate::db::Database;

/// Shared application data accessible in all Discord command contexts and event handlers.
#[derive(Clone)]
pub struct AppData {
    pub config: Arc<Config>,
    pub db: Database,
    pub http: reqwest::Client,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, AppData, Error>;

pub use client::run;
