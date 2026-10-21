use poise::serenity_prelude as serenity;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{error, info, warn};

use super::client::{start_syncplay_client, SyncplayClientConfig, SyncplayClientHandle, SyncplayStatus};
use super::protocol::BridgeEvent;
use crate::db::Database;

/// Coordinates bidirectional relay between Discord and Syncplay server.
#[derive(Clone)]
pub struct SyncplayBridgeHandle {
    client: SyncplayClientHandle,
    bound_channel_id: Arc<AtomicU64>,
    active_room: Arc<RwLock<String>>,
    db: Database,
}

impl SyncplayBridgeHandle {
    /// Returns the currently bound Discord channel ID, if any.
    pub fn get_bound_channel_id(&self) -> Option<u64> {
        let id = self.bound_channel_id.load(Ordering::Relaxed);
        if id == 0 {
            None
        } else {
            Some(id)
        }
    }

    /// Sets the bound Discord channel and optionally switches room.
    pub async fn bind_channel(
        &self,
        channel_id: u64,
        room: &str,
        password: Option<&str>,
    ) -> anyhow::Result<()> {
        self.bound_channel_id.store(channel_id, Ordering::Relaxed);
        {
            let mut r = self.active_room.write().await;
            *r = room.to_string();
        }

        // Persist binding in database
        let _ = self.db.set_setting("syncplay_channel_id", &channel_id.to_string()).await;
        let _ = self.db.set_setting("syncplay_room", room).await;

        self.client.switch_room(room, password).await?;
        Ok(())
    }

    /// Unbinds the Discord channel from Syncplay.
    pub async fn unbind_channel(&self) -> anyhow::Result<()> {
        self.bound_channel_id.store(0, Ordering::Relaxed);
        let _ = self.db.set_setting("syncplay_channel_id", "0").await;
        Ok(())
    }

    /// Switches the active Syncplay room.
    pub async fn switch_room(&self, room: &str, password: Option<&str>) -> anyhow::Result<()> {
        {
            let mut r = self.active_room.write().await;
            *r = room.to_string();
        }
        let _ = self.db.set_setting("syncplay_room", room).await;
        self.client.switch_room(room, password).await
    }

    /// Relays a Discord chat message to the Syncplay room.
    pub async fn send_discord_chat(&self, author_name: &str, content: &str) -> anyhow::Result<()> {
        let clean = content.trim();
        if clean.is_empty() {
            return Ok(());
        }

        // Limit length to respect MAX_CHAT_MESSAGE (150 chars in Syncplay)
        let mut formatted = format!("[{author_name}]: {clean}");
        if formatted.len() > 145 {
            formatted.truncate(142);
            formatted.push_str("...");
        }

        self.client.send_chat(&formatted).await
    }

    /// Gets current live status of the Syncplay connection and room.
    pub async fn get_status(&self) -> SyncplayStatus {
        self.client.get_status()
    }
}

/// Starts the Syncplay bridge service.
pub async fn start_syncplay_bridge(
    initial_config: SyncplayClientConfig,
    initial_channel_id: Option<u64>,
    db: Database,
    http: Arc<serenity::Http>,
) -> SyncplayBridgeHandle {
    // Check if database has saved channel_id or room overrides
    let saved_channel = db
        .get_setting("syncplay_channel_id")
        .await
        .ok()
        .flatten()
        .and_then(|id| id.parse::<u64>().ok())
        .or(initial_channel_id)
        .unwrap_or(0);

    let saved_room = db
        .get_setting("syncplay_room")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| initial_config.room.clone());

    let mut client_config = initial_config;
    client_config.room = saved_room.clone();

    let bound_channel_id = Arc::new(AtomicU64::new(saved_channel));
    let active_room = Arc::new(RwLock::new(saved_room));

    let (event_tx, event_rx) = mpsc::channel(100);
    let client_handle = start_syncplay_client(client_config, event_tx);

    let bridge_handle = SyncplayBridgeHandle {
        client: client_handle,
        bound_channel_id: bound_channel_id.clone(),
        active_room: active_room.clone(),
        db,
    };

    tokio::spawn(run_bridge_event_processor(event_rx, bound_channel_id, http));

    bridge_handle
}

async fn run_bridge_event_processor(
    mut event_rx: mpsc::Receiver<BridgeEvent>,
    bound_channel_id: Arc<AtomicU64>,
    http: Arc<serenity::Http>,
) {
    let last_playstate_notify = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10)));

    while let Some(event) = event_rx.recv().await {
        let channel_snowflake = bound_channel_id.load(Ordering::Relaxed);
        if channel_snowflake == 0 {
            continue;
        }

        let channel = serenity::ChannelId::new(channel_snowflake);

        match event {
            BridgeEvent::Chat { username, room, message } => {
                if let Err(e) = relay_chat_to_discord(&http, channel, &username, &room, &message).await {
                    error!("Failed to relay Syncplay chat to Discord channel {channel_snowflake}: {e:?}");
                }
            }
            BridgeEvent::UserJoined { username, room } => {
                let msg = format!("👋 **{username}** joined room `#{room}`.");
                let _ = channel.say(&http, msg).await;
            }
            BridgeEvent::UserLeft { username, room } => {
                let msg = format!("🚪 **{username}** left room `#{room}`.");
                let _ = channel.say(&http, msg).await;
            }
            BridgeEvent::FileLoaded { username, room, filename, duration } => {
                let dur_str = format_duration(duration);
                let msg = if duration > 0.0 {
                    format!("🎬 **{username}** in `#{room}` loaded file: `{filename}` ({dur_str})")
                } else {
                    format!("🎬 **{username}** in `#{room}` loaded file: `{filename}`")
                };
                let _ = channel.say(&http, msg).await;
            }
            BridgeEvent::PlaystateChanged { room, paused, position, set_by } => {
                let mut last = last_playstate_notify.lock().await;
                // Throttling: ignore rapid consecutive playstate notifications (max 1 per 5s)
                if last.elapsed() < Duration::from_secs(5) {
                    continue;
                }
                *last = Instant::now();

                let time_str = format_duration(position);
                let msg = if paused {
                    format!("⏸️ **{set_by}** paused in `#{room}` at `{time_str}`.")
                } else {
                    format!("▶️ **{set_by}** resumed in `#{room}` at `{time_str}`.")
                };
                let _ = channel.say(&http, msg).await;
            }
            BridgeEvent::Connected { room, .. } => {
                info!("Syncplay bridge active and listening in room '{room}'.");
            }
            BridgeEvent::Disconnected { reason } => {
                warn!("Syncplay bridge disconnected: {reason}");
            }
        }
    }
}

/// Relays a chat message into Discord using Webhook impersonation, or fallback to standard message.
async fn relay_chat_to_discord(
    http: &serenity::Http,
    channel_id: serenity::ChannelId,
    username: &str,
    room: &str,
    message: &str,
) -> anyhow::Result<()> {
    // Attempt webhook execution for native avatar and username display
    let webhooks_res = channel_id.webhooks(http).await;
    if let Ok(webhooks) = webhooks_res {
        let webhook = if let Some(existing) = webhooks.into_iter().find(|w| w.name.as_deref() == Some("Syncplay Bridge")) {
            Some(existing)
        } else {
            channel_id
                .create_webhook(http, serenity::CreateWebhook::new("Syncplay Bridge"))
                .await
                .ok()
        };

        if let Some(wh) = webhook {
            let avatar = format!("https://api.dicebear.com/7.x/bottts/png?seed={username}");
            let exec_result = wh
                .execute(
                    http,
                    false,
                    serenity::ExecuteWebhook::new()
                        .username(format!("{username} (Syncplay - #{room})"))
                        .avatar_url(avatar)
                        .content(message),
                )
                .await;

            if exec_result.is_ok() {
                return Ok(());
            }
        }
    }

    // Fallback: Post standard bot message
    channel_id
        .say(http, format!("**[{username} in #{room}]**: {message}"))
        .await?;

    Ok(())
}

fn format_duration(seconds: f64) -> String {
    let total_secs = seconds.max(0.0) as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

