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
use crate::instagram::models::{
    ExtractedMedia, InboundInstagramEvent, MetaVerificationQuery, MetaWebhookPayload,
};

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
    Json(payload_value): Json<serde_json::Value>,
) -> impl IntoResponse {
    info!(
        raw_payload = %payload_value,
        "Inbound Instagram webhook received"
    );

    let payload: MetaWebhookPayload = match serde_json::from_value(payload_value) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to deserialize Meta webhook payload: {e}");
            return (StatusCode::OK, "EVENT_RECEIVED");
        }
    };

    if payload.object.as_deref() != Some("instagram") {
        return (StatusCode::NOT_FOUND, "Not an instagram object");
    }

    let events = payload.extract_events();
    info!("Extracted {} event(s) from webhook", events.len());

    for event in events {
        let state_clone = state.clone();
        tokio::spawn(async move {
            match event {
                InboundInstagramEvent::Media(media) => {
                    process_and_deliver_media(state_clone, media).await;
                }
                InboundInstagramEvent::Text { sender_id, text } => {
                    process_and_deliver_text(state_clone, sender_id, text).await;
                }
            }
        });
    }

    (StatusCode::OK, "EVENT_RECEIVED")
}

/// Resolves user display name with mention formatting if mapped.
async fn resolve_sender_display(state: &WebhookState, sender_id: &str) -> String {
    match state.db.get_instagram_sender(sender_id).await {
        Ok(Some(sender)) => {
            if let Some(discord_id) = sender.discord_user_id {
                format!("<@{discord_id}> (@{})", sender.username)
            } else {
                format!("@{}", sender.username)
            }
        }
        _ => {
            let last4 = if sender_id.len() >= 4 {
                &sender_id[sender_id.len() - 4..]
            } else {
                sender_id
            };
            format!("User_{last4}")
        }
    }
}

/// Resolves the configured Discord target channel ID.
async fn resolve_target_channel(state: &WebhookState) -> Option<serenity::ChannelId> {
    let channel_id_u64 = match state.db.get_setting("instagram_target_channel_id").await {
        Ok(Some(id_str)) => id_str.parse::<u64>().ok(),
        _ => None,
    }
    .or(state.default_target_channel_id);

    channel_id_u64.map(serenity::ChannelId::new)
}

/// Processes an inbound Instagram text message and posts it directly to Discord.
async fn process_and_deliver_text(state: WebhookState, sender_id: String, text: String) {
    info!(
        sender_id = %sender_id,
        text = %text,
        "Delivering inbound Instagram text message"
    );

    let target_channel_id = match resolve_target_channel(&state).await {
        Some(ch) => ch,
        None => {
            error!("Target channel ID not configured. Use /instagram channel set");
            return;
        }
    };

    let sender_display = resolve_sender_display(&state, &sender_id).await;
    let content = format!("{sender_display}: {text}");

    let msg = serenity::CreateMessage::new().content(content);
    if let Err(e) = target_channel_id.send_message(&state.http, msg).await {
        error!("Failed to post text message to Discord: {e}");
    } else {
        info!("Successfully delivered Instagram text message to Discord");
    }
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

    let target_channel_id = match resolve_target_channel(&state).await {
        Some(ch) => ch,
        None => {
            error!("Target channel ID not configured. Use /instagram channel set");
            return;
        }
    };

    let sender_display = resolve_sender_display(&state, &media.sender_id).await;

    // Construct caption header: <@discord_id> (@username): text, or <@discord_id> (@username)
    let content_text = match &media.user_text {
        Some(caption) => format!("{sender_display}: {caption}"),
        None => sender_display,
    };

    // Download or Prepare Media
    let download_result = downloader::download_media(&media.media_url).await;

    // Construct button only if this is a genuine Instagram URL (not internal CDN)
    let mut components = Vec::new();
    if crate::instagram::models::is_instagram_url(&media.media_url) {
        let link_button = serenity::CreateButton::new_link(&media.media_url)
            .label(media.media_kind.button_label());
        components.push(serenity::CreateActionRow::Buttons(vec![link_button]));
    }

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
                let mut msg = serenity::CreateMessage::new()
                    .content(content_text)
                    .files(attachments);

                if !components.is_empty() {
                    msg = msg.components(components.clone());
                }

                if let Err(e) = target_channel_id.send_message(&state.http, msg).await {
                    error!("Failed to send direct media files to Discord channel: {e}");
                } else {
                    info!("Successfully uploaded media files to Discord");
                }
            } else if crate::instagram::models::is_instagram_url(&media.media_url) {
                let fallback_url = downloader::to_fxinstagram_url(&media.media_url);
                let mut msg = serenity::CreateMessage::new()
                    .content(format!("{content_text}\n{fallback_url}"));
                if !components.is_empty() {
                    msg = msg.components(components);
                }
                let _ = target_channel_id.send_message(&state.http, msg).await;
            }

            downloader::cleanup_files(&paths).await;
        }
        MediaDownload::TooLarge { fallback_url, .. } | MediaDownload::Failed { fallback_url, .. } => {
            if crate::instagram::models::is_instagram_url(&media.media_url) {
                let mut msg = serenity::CreateMessage::new()
                    .content(format!("{content_text}\n{fallback_url}"));

                if !components.is_empty() {
                    msg = msg.components(components);
                }

                if let Err(e) = target_channel_id.send_message(&state.http, msg).await {
                    error!("Failed to post media link to Discord: {e}");
                } else {
                    info!("Posted media stream to Discord");
                }
            } else {
                warn!("CDN media download was not usable, skipping link fallback");
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
