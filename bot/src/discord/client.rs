use std::sync::Arc;
use poise::serenity_prelude as serenity;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::db::Database;
use crate::discord::{commands, events, AppData, Error};
use crate::embeddings::EmbeddingEngine;
use crate::error::AppError;

/// Builds and starts the Discord bot client with Poise framework and Serenity.
pub async fn run(
    config: Arc<Config>,
    db: Database,
    embeddings: Arc<EmbeddingEngine>,
) -> Result<(), AppError> {
    let framework_options = poise::FrameworkOptions {
        commands: commands::all(),
        event_handler: |framework, event| {
            Box::pin(events::handle_event(framework, event))
        },
        on_error: |error| Box::pin(handle_framework_error(error)),
        pre_command: |ctx| {
            Box::pin(async move {
                info!(
                    "Executing command '/{}' invoked by user {} in channel {}",
                    ctx.command().name,
                    ctx.author().id,
                    ctx.channel_id()
                );
            })
        },
        post_command: |ctx| {
            Box::pin(async move {
                info!(
                    "Successfully executed command '/{}' invoked by user {}",
                    ctx.command().name,
                    ctx.author().id
                );
            })
        },
        ..Default::default()
    };

    let token = config.discord_token.clone();
    let config_for_app = config.clone();
    let config_for_webhook = config.clone();
    let db_for_app = db.clone();
    let db_for_webhook = db.clone();
    let embeddings_for_app = embeddings.clone();

    let framework = poise::Framework::builder()
        .options(framework_options)
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                let commands = &framework.options().commands;

                // 1. Register commands globally (updates all servers and DMs)
                if let Err(e) = poise::builtins::register_globally(ctx, commands).await {
                    error!("Failed to register slash commands globally: {e:?}");
                } else {
                    info!("Successfully registered slash commands globally");
                }

                // 2. Clear any lingering guild-scoped commands in all servers to eliminate duplicates
                let empty_cmds: &[poise::Command<AppData, Error>] = &[];
                let mut guild_ids: Vec<serenity::model::id::GuildId> =
                    ready.guilds.iter().map(|g| g.id).collect();

                if let Some(dev_guild) = config_for_app.dev_guild_id {
                    if !guild_ids.contains(&dev_guild) {
                        guild_ids.push(dev_guild);
                    }
                }

                for guild_id in guild_ids {
                    if let Err(e) = poise::builtins::register_in_guild(ctx, empty_cmds, guild_id).await {
                        warn!("Failed to clear legacy guild slash commands in server {guild_id}: {e:?}");
                    } else {
                        info!("Cleared guild-scoped slash commands for server {guild_id} to prevent duplicates");
                    }
                }

                let http = reqwest::Client::builder()
                    .user_agent("DiscordAiBot/0.1.0")
                    .build()
                    .unwrap_or_default();

                Ok(AppData {
                    config: config_for_app,
                    db: db_for_app,
                    http,
                    embeddings: embeddings_for_app,
                })
            })
        })
        .build();

    // Gateway intents:
    // Configured for message collection readiness:
    // - GUILDS: Necessary for guild state and channel mappings
    // - GUILD_MESSAGES: Necessary to observe guild chat events
    // - DIRECT_MESSAGES: Supports direct messages if needed
    // - MESSAGE_CONTENT: Privileged intent required to read message text
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::DIRECT_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .map_err(AppError::Discord)?;

    // Spawn the Instagram Webhook Server
    let webhook_state = crate::instagram::WebhookState {
        http: client.http.clone(),
        db: db_for_webhook,
        verify_token: config_for_webhook.instagram_verify_token.clone(),
        default_target_channel_id: config_for_webhook.instagram_target_channel_id,
    };
    let webhook_port = config_for_webhook.instagram_webhook_port;

    tokio::spawn(async move {
        if let Err(e) = crate::instagram::start_webhook_server(webhook_state, webhook_port).await {
            error!("Instagram webhook server error: {e}");
        }
    });

    info!("Connecting to Discord gateway...");
    client.start().await.map_err(AppError::Discord)?;

    Ok(())
}

async fn handle_framework_error(error: poise::FrameworkError<'_, AppData, Error>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => {
            error!("Failed to complete bot framework setup: {error:?}");
        }
        poise::FrameworkError::Command { error, ctx, .. } => {
            error!(
                "Command error in '/{}' by user {}: {error:?}",
                ctx.command().name,
                ctx.author().id
            );
            let _ = ctx
                .say(format!("❌ **Command Error**: {error}"))
                .await;
        }
        poise::FrameworkError::CommandCheckFailed { error, ctx, .. } => {
            if let Some(err) = error {
                warn!(
                    "Check failed on command '/{}' by user {}: {err:?}",
                    ctx.command().name,
                    ctx.author().id
                );
            }
        }
        other => {
            if let Err(e) = poise::builtins::on_error(other).await {
                error!("Unhandled framework error: {e:?}");
            }
        }
    }
}
