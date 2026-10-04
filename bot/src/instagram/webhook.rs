use std::sync::Arc;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use poise::serenity_prelude as serenity;
use tracing::{error, info, warn};

use crate::db::Database;
use crate::instagram::downloader::{self, ReelDownload};
use crate::instagram::models::{ExtractedReel, MetaVerificationQuery, MetaWebhookPayload};

pub const INSTAGRAM_MAGENTA: u32 = 0xE1306C;
pub const INSTAGRAM_ICON_URL: &str =
    "https://upload.wikimedia.org/wikipedia/commons/thumb/a/a5/Instagram_icon.png/600px-Instagram_icon.png";

#[derive(Clone)]
pub struct WebhookState {
    pub http: Arc<serenity::Http>,
    pub db: Database,
    pub verify_token: String,
    pub default_target_channel_id: Option<u64>,
}

/// Router for the Instagram Meta webhook endpoints.
pub fn create_webhook_router(state: WebhookState) -> Router {
    Router::new()
        .route("/webhook", get(verify_webhook))
        .route("/webhook", post(handle_webhook))
        .with_state(state)
}

/// Meta Webhook Verification handler (GET /webhook).
async fn verify_webhook(
    State(state): State<WebhookState>,
    Query(query): Query<MetaVerificationQuery>,
) -> impl IntoResponse {
    let mode = query.mode.as_deref().unwrap_or_default();
    let token = query.verify_token.as_deref().unwrap_or_default();

    if mode == "subscribe" && token == state.verify_token {
        info!("Meta Webhook successfully verified and linked!");
        let challenge = query.challenge.unwrap_or_default();
        (StatusCode::OK, challenge)
    } else {
        warn!(
            mode,
            received_token = token,
            "Meta Webhook verification failed: token mismatch or invalid mode"
        );
        (StatusCode::FORBIDDEN, "Forbidden".to_string())
    }
}

/// Meta Webhook Event ingestion handler (POST /webhook).
async fn handle_webhook(
    State(state): State<WebhookState>,
    Json(payload): Json<MetaWebhookPayload>,
) -> impl IntoResponse {
    if payload.object.as_deref() != Some("instagram") {
        return (StatusCode::NOT_FOUND, "Not an instagram object");
    }

    let reels = payload.extract_reels();
    info!("Received webhook with {} reel event(s)", reels.len());

    for reel in reels {
        let state_clone = state.clone();
        tokio::spawn(async move {
            process_and_deliver_reel(state_clone, reel).await;
        });
    }

    (StatusCode::OK, "EVENT_RECEIVED")
}

/// Processes an extracted Instagram reel and posts it to Discord.
async fn process_and_deliver_reel(state: WebhookState, reel: ExtractedReel) {
    info!(
        sender_id = %reel.sender_id,
        reel_url = %reel.reel_url,
        "Processing inbound Instagram reel for Discord delivery"
    );

    // 1. Resolve Target Channel ID
    let channel_id_u64 = match state.db.get_setting("instagram_target_channel_id").await {
        Ok(Some(id_str)) => id_str.parse::<u64>().ok(),
        _ => None,
    }
    .or(state.default_target_channel_id);

    let target_channel_id = match channel_id_u64 {
        Some(id) => serenity::ChannelId::new(id),
        None => {
            error!(
                "Cannot deliver Instagram reel: target channel ID not configured. \
                Use /instagram channel set or configure INSTAGRAM_TARGET_CHANNEL_ID in .env"
            );
            return;
        }
    };

    // 2. Resolve Sender Profile
    let (sender_name, discord_tag) = match state.db.get_instagram_sender(&reel.sender_id).await {
        Ok(Some(sender)) => {
            let name = format!("@{}", sender.username);
            let tag = sender.discord_user_id.map(|id| format!(" (<@{id}>)"));
            (name, tag)
        }
        _ => {
            let last4 = if reel.sender_id.len() >= 4 {
                &reel.sender_id[reel.sender_id.len() - 4..]
            } else {
                &reel.sender_id
            };
            (format!("User_{last4}"), None)
        }
    };

    // 3. Download or Prepare Media
    let download_result = downloader::download_reel(&reel.reel_url).await;

    // 4. Construct Modern Discord Message & Embed
    let link_button = serenity::CreateButton::new_link(&reel.reel_url).label("Watch on Instagram");
    let action_row = serenity::CreateActionRow::Buttons(vec![link_button]);

    let author_text = format!("Reel from {sender_name}");
    let mention_part = discord_tag.unwrap_or_default();

    match download_result {
        ReelDownload::DirectFile { path, size_mb } => {
            let embed = serenity::CreateEmbed::new()
                .author(
                    serenity::CreateEmbedAuthor::new(author_text)
                        .icon_url(INSTAGRAM_ICON_URL)
                        .url(&reel.reel_url),
                )
                .title("New Instagram Reel")
                .url(&reel.reel_url)
                .description(format!("Shared by **{sender_name}**{mention_part}"))
                .color(INSTAGRAM_MAGENTA)
                .footer(serenity::CreateEmbedFooter::new(format!(
                    "Direct Upload • {size_mb:.1} MB"
                )));

            match serenity::CreateAttachment::path(&path).await {
                Ok(attachment) => {
                    let msg = serenity::CreateMessage::new()
                        .embed(embed)
                        .components(vec![action_row])
                        .files(vec![attachment]);

                    if let Err(e) = target_channel_id.send_message(&state.http, msg).await {
                        error!("Failed to send direct reel video to Discord channel: {e}");
                    } else {
                        info!("Successfully uploaded reel video file to Discord!");
                    }
                }
                Err(e) => {
                    error!("Failed to create attachment from reel file: {e}");
                    // Fallback to proxy link
                    let fallback_url = downloader::to_ddinstagram_url(&reel.reel_url);
                    let fallback_msg = serenity::CreateMessage::new()
                        .content(format!("**New Reel from {sender_name}:**\n{fallback_url}"))
                        .components(vec![action_row]);
                    let _ = target_channel_id.send_message(&state.http, fallback_msg).await;
                }
            }

            downloader::cleanup_file(&path).await;
        }
        ReelDownload::TooLarge {
            size_mb,
            fallback_url,
        } => {
            let embed = serenity::CreateEmbed::new()
                .author(
                    serenity::CreateEmbedAuthor::new(author_text)
                        .icon_url(INSTAGRAM_ICON_URL)
                        .url(&reel.reel_url),
                )
                .title("New Instagram Reel")
                .url(&reel.reel_url)
                .description(format!(
                    "Shared by **{sender_name}**{mention_part}\n\n*Video exceeds 25MB limit ({size_mb:.1} MB). Streaming via inline proxy:*",
                ))
                .color(INSTAGRAM_MAGENTA)
                .footer(serenity::CreateEmbedFooter::new(format!(
                    "Proxy Player • {size_mb:.1} MB"
                )));

            let msg = serenity::CreateMessage::new()
                .content(fallback_url)
                .embed(embed)
                .components(vec![action_row]);

            if let Err(e) = target_channel_id.send_message(&state.http, msg).await {
                error!("Failed to post proxy reel link to Discord: {e}");
            } else {
                info!("Posted proxy-embedded reel to Discord!");
            }
        }
        ReelDownload::Failed {
            reason,
            fallback_url,
        } => {
            warn!(reason, "Reel download failed; using proxy fallback");
            let embed = serenity::CreateEmbed::new()
                .author(
                    serenity::CreateEmbedAuthor::new(author_text)
                        .icon_url(INSTAGRAM_ICON_URL)
                        .url(&reel.reel_url),
                )
                .title("New Instagram Reel")
                .url(&reel.reel_url)
                .description(format!(
                    "Shared by **{sender_name}**{mention_part}\n\n*Stream preview:*",
                ))
                .color(INSTAGRAM_MAGENTA)
                .footer(serenity::CreateEmbedFooter::new("Proxy Stream Preview"));

            let msg = serenity::CreateMessage::new()
                .content(fallback_url)
                .embed(embed)
                .components(vec![action_row]);

            if let Err(e) = target_channel_id.send_message(&state.http, msg).await {
                error!("Failed to post fallback reel to Discord: {e}");
            } else {
                info!("Posted fallback reel stream to Discord!");
            }
        }
    }
}

/// Starts the Axum HTTP server on the given port.
pub async fn start_webhook_server(
    state: WebhookState,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = create_webhook_router(state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    info!("🚀 Webhook gateway listening on http://0.0.0.0:{port}/webhook");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
