use std::fmt;
use serenity::model::id::{GuildId, UserId};
use crate::error::{ConfigError, Result};

#[derive(Clone)]
pub struct Config {
    pub discord_token: String,
    pub database_url: String,
    pub owner_id: UserId,
    pub target_user_id: UserId,
    pub dev_guild_id: Option<GuildId>,
    pub rust_log: String,
    pub instagram_verify_token: String,
    pub instagram_webhook_port: u16,
    pub instagram_target_channel_id: Option<u64>,
}

impl Config {
    /// Loads and validates configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let discord_token = std::env::var("DISCORD_TOKEN")
            .map_err(|_| ConfigError::MissingVariable("DISCORD_TOKEN".to_string()))?;
        if discord_token.trim().is_empty() {
            return Err(ConfigError::InvalidValue {
                name: "DISCORD_TOKEN".to_string(),
                reason: "Token cannot be empty".to_string(),
            }
            .into());
        }

        let mut database_url = match std::env::var("DATABASE_URL") {
            Ok(url) if !url.trim().is_empty() => url,
            _ => {
                let user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "postgres".to_string());
                let password = std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "postgres".to_string());
                let db = std::env::var("POSTGRES_DB").unwrap_or_else(|_| "discord_ai".to_string());
                format!("postgresql://{user}:{password}@postgres:5432/{db}")
            }
        };

        // If DATABASE_URL contains unresolved env var placeholders like ${POSTGRES_USER}
        if database_url.contains("${") {
            let user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "postgres".to_string());
            let password = std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "postgres".to_string());
            let db = std::env::var("POSTGRES_DB").unwrap_or_else(|_| "discord_ai".to_string());
            database_url = database_url
                .replace("${POSTGRES_USER}", &user)
                .replace("${POSTGRES_PASSWORD}", &password)
                .replace("${POSTGRES_DB}", &db);
        }

        // Automatic local development fallback:
        // If DATABASE_URL points to '@postgres:' but 'postgres' cannot be resolved
        // (common when running cargo run directly on the host machine), fallback to localhost.
        if database_url.contains("@postgres:") {
            use std::net::ToSocketAddrs;
            if "postgres:5432".to_socket_addrs().is_err() {
                let host_port = std::env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
                tracing::info!(
                    "Host 'postgres' is unreachable directly; falling back to localhost:{host_port} for local dev"
                );
                database_url = database_url.replace("@postgres:5432", &format!("@localhost:{host_port}"));
                database_url = database_url.replace("@postgres:", &format!("@localhost:{host_port}"));
            }
        }

        let owner_id_raw = std::env::var("OWNER_ID")
            .map_err(|_| ConfigError::MissingVariable("OWNER_ID".to_string()))?;
        let owner_id_val = owner_id_raw.trim().parse::<u64>().map_err(|e| {
            ConfigError::InvalidValue {
                name: "OWNER_ID".to_string(),
                reason: format!("Failed to parse as valid u64 Discord ID: {e}"),
            }
        })?;
        let owner_id = UserId::new(owner_id_val);

        let target_user_id_raw = std::env::var("TARGET_USER_ID")
            .map_err(|_| ConfigError::MissingVariable("TARGET_USER_ID".to_string()))?;
        let target_user_id_val = target_user_id_raw.trim().parse::<u64>().map_err(|e| {
            ConfigError::InvalidValue {
                name: "TARGET_USER_ID".to_string(),
                reason: format!("Failed to parse as valid u64 Discord ID: {e}"),
            }
        })?;
        let target_user_id = UserId::new(target_user_id_val);

        let dev_guild_id = match std::env::var("DEV_GUILD_ID") {
            Ok(val) if !val.trim().is_empty() => {
                let id = val.trim().parse::<u64>().map_err(|e| ConfigError::InvalidValue {
                    name: "DEV_GUILD_ID".to_string(),
                    reason: format!("Failed to parse as valid u64 Discord ID: {e}"),
                })?;
                Some(GuildId::new(id))
            }
            _ => None,
        };

        let rust_log = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

        let instagram_verify_token = std::env::var("INSTAGRAM_VERIFY_TOKEN")
            .or_else(|_| std::env::var("VERIFY_TOKEN"))
            .unwrap_or_default();

        let instagram_webhook_port = std::env::var("INSTAGRAM_WEBHOOK_PORT")
            .or_else(|_| std::env::var("PORT"))
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(3000);

        let instagram_target_channel_id = std::env::var("INSTAGRAM_TARGET_CHANNEL_ID")
            .or_else(|_| std::env::var("TARGET_CHANNEL_ID"))
            .ok()
            .and_then(|id| id.parse::<u64>().ok());

        Ok(Self {
            discord_token,
            database_url,
            owner_id,
            target_user_id,
            dev_guild_id,
            rust_log,
            instagram_verify_token,
            instagram_webhook_port,
            instagram_target_channel_id,
        })
    }

    /// Checks if a given Discord user ID matches the configured bot owner.
    pub fn is_owner(&self, user_id: UserId) -> bool {
        self.owner_id == user_id
    }

    /// Checks if a given Discord user ID matches the target user to be modeled.
    #[allow(dead_code)]
    pub fn is_target_user(&self, user_id: UserId) -> bool {
        self.target_user_id == user_id
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("discord_token", &"[REDACTED]")
            .field("database_url", &"[REDACTED]")
            .field("owner_id", &self.owner_id)
            .field("target_user_id", &self.target_user_id)
            .field("dev_guild_id", &self.dev_guild_id)
            .field("rust_log", &self.rust_log)
            .field("instagram_verify_token", &"[REDACTED]")
            .field("instagram_webhook_port", &self.instagram_webhook_port)
            .field("instagram_target_channel_id", &self.instagram_target_channel_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_redaction_in_debug() {
        let config = Config {
            discord_token: "super_secret_discord_token".to_string(),
            database_url: "postgresql://user:pass@localhost:5432/db".to_string(),
            owner_id: UserId::new(111222333),
            target_user_id: UserId::new(444555666),
            dev_guild_id: Some(GuildId::new(777888999)),
            rust_log: "info".to_string(),
            instagram_verify_token: "super_secret_verify_token".to_string(),
            instagram_webhook_port: 3000,
            instagram_target_channel_id: Some(12345),
        };

        let debug_str = format!("{config:?}");
        assert!(!debug_str.contains("super_secret_discord_token"));
        assert!(!debug_str.contains("postgresql://user:pass"));
        assert!(!debug_str.contains("super_secret_verify_token"));
        assert!(debug_str.contains("[REDACTED]"));
        assert!(debug_str.contains("111222333"));
        assert!(debug_str.contains("444555666"));
        assert!(debug_str.contains("777888999"));
        assert!(debug_str.contains("3000"));
        assert!(debug_str.contains("12345"));
    }

    #[test]
    fn test_owner_and_target_id_checks() {
        let config = Config {
            discord_token: "dummy".to_string(),
            database_url: "dummy".to_string(),
            owner_id: UserId::new(100),
            target_user_id: UserId::new(200),
            dev_guild_id: None,
            rust_log: "info".to_string(),
            instagram_verify_token: "dummy".to_string(),
            instagram_webhook_port: 3000,
            instagram_target_channel_id: None,
        };

        assert!(config.is_owner(UserId::new(100)));
        assert!(!config.is_owner(UserId::new(999)));

        assert!(config.is_target_user(UserId::new(200)));
        assert!(!config.is_target_user(UserId::new(100)));
    }
}
