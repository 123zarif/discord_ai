use std::sync::Arc;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use poise::serenity_prelude as serenity;
use tracing::{error, info, warn};

use crate::db::Database;
use crate::instagram::downloader::{self, MediaDownload};
use crate::instagram::models::{ExtractedMedia, MetaVerificationQuery, MetaWebhookPayload};

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
        info!("Meta Webhook verified successfully");
        let challenge = query.challenge.unwrap_or_default();
        (StatusCode::OK, challenge)
    } else {
        warn!(
            mode,
            received_token = token,
            "Meta Webhook verification failed: token mismatch"
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

    let items = payload.extract_media();
    info!("Received webhook with {} media item(s)", items.len());

    for item in items {
        let state_clone = state.clone();
        tokio::spawn(async move {
            process_and_deliver_media(state_clone, item).await;
        });
    }

    (StatusCode::OK, "EVENT_RECEIVED")
}

/// Processes an extracted Instagram media item (reel, post, or collection) and posts it to Discord.
async fn process_and_deliver_media(state: WebhookState, media: ExtractedMedia) {
    info!(
        sender_id = %media.sender_id,
        media_url = %media.media_url,
        kind = ?media.media_kind,
        has_caption = media.user_text.is_some(),
        "Processing Instagram media for delivery"
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
                "Target channel ID not configured. Use /instagram channel set or set INSTAGRAM_TARGET_CHANNEL_ID"
            );
            return;
        }
    };

    // 2. Resolve Sender Profile
    let sender_display = match state.db.get_instagram_sender(&media.sender_id).await {
        Ok(Some(sender)) => {
            if let Some(discord_id) = sender.discord_user_id {
                format!("<@{discord_id}> (@{})", sender.username)
            } else {
                format!("@{}", sender.username)
            }
        }
        _ => {
            let last4 = if media.sender_id.len() >= 4 {
                &media.sender_id[media.sender_id.len() - 4..]
            } else {
                &media.sender_id
            };
            format!("User_{last4}")
        }
    };

    // Construct caption header
    let content_text = match &media.user_text {
        Some(caption) => format!("{sender_display}: {caption}"),
        None => sender_display,
    };

    // 3. Download or Prepare Media
    let download_result = downloader::download_media(&media.media_url).await;

    // 4. Construct button
    let link_button = serenity::CreateButton::new_link(&media.media_url)
        .label(media.media_kind.button_label());
    let action_row = serenity::CreateActionRow::Buttons(vec![link_button]);

    match download_result {
        MediaDownload::DirectFiles { paths, .. } => {
            let mut attachments = Vec::new();
            for path in &paths {
                match serenity::CreateAttachment::path(path).await {
                    Ok(att) => attachments.push(att),
                    Err(e) => warn!("Failed to load attachment from {:?}: {e}", path),
                }
            }

            if !attachments.is_empty() {
                let msg = serenity::CreateMessage::new()
                    .content(content_text)
                    .components(vec![action_row])
                    .files(attachments);

                if let Err(e) = target_channel_id.send_message(&state.http, msg).await {
                    error!("Failed to send direct media files to Discord channel: {e}");
                } else {
                    info!("Successfully uploaded media files to Discord");
                }
            } else {
                let fallback_url = downloader::to_ddinstagram_url(&media.media_url);
                let msg = serenity::CreateMessage::new()
                    .content(format!("{content_text}\n{fallback_url}"))
                    .components(vec![action_row]);
                let _ = target_channel_id.send_message(&state.http, msg).await;
            }

            downloader::cleanup_files(&paths).await;
        }
        MediaDownload::TooLarge { fallback_url, .. } | MediaDownload::Failed { fallback_url, .. } => {
            let msg = serenity::CreateMessage::new()
                .content(format!("{content_text}\n{fallback_url}"))
                .components(vec![action_row]);

            if let Err(e) = target_channel_id.send_message(&state.http, msg).await {
                error!("Failed to post media link to Discord: {e}");
            } else {
                info!("Posted media stream to Discord");
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
    info!("Webhook gateway listening on http://0.0.0.0:{port}/webhook");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
