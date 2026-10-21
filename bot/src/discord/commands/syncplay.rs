use poise::serenity_prelude as serenity;
use crate::discord::{Context, Error};

const COLOR_SYNCPLAY: u32 = 0x5865F2; // Syncplay Blurple

/// Manage the Syncplay server chat and playback bridge.
#[poise::command(
    slash_command,
    subcommands("bind", "unbind", "status", "room", "send"),
    subcommand_required
)]
pub async fn syncplay(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Bind a Discord channel to a Syncplay room for bidirectional chat.
#[poise::command(slash_command)]
pub async fn bind(
    ctx: Context<'_>,
    #[description = "Discord channel to bind to Syncplay chat"]
    channel: serenity::Channel,
    #[description = "Syncplay room name (e.g. cinema)"]
    room: String,
    #[description = "Optional room password"]
    password: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let channel_id = channel.id().get();
    let room_clean = room.trim();

    ctx.data()
        .syncplay
        .bind_channel(channel_id, room_clean, password.as_deref())
        .await?;

    let embed = serenity::CreateEmbed::new()
        .title("Syncplay Bridge Bound")
        .description(format!(
            "Successfully bound <#{channel_id}> to Syncplay room **`{room_clean}`**.\n\n\
            • Messages sent in <#{channel_id}> will relay to Syncplay.\n\
            • Syncplay chat and room events will appear here.\n\
            • Use `/syncplay status` to check active viewers."
        ))
        .color(COLOR_SYNCPLAY);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Unbind the Discord channel from Syncplay.
#[poise::command(slash_command)]
pub async fn unbind(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    ctx.data().syncplay.unbind_channel().await?;

    let embed = serenity::CreateEmbed::new()
        .title("Syncplay Bridge Unbound")
        .description("The Syncplay chat bridge has been unbound. Messages will no longer be relayed.")
        .color(0x95A5A6);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// View Syncplay server connection status, active room, and current viewers.
#[poise::command(slash_command)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let status = ctx.data().syncplay.get_status().await;
    let bound_channel = ctx.data().syncplay.get_bound_channel_id();

    let conn_str = if status.is_connected {
        "🟢 `Connected`"
    } else {
        "🔴 `Disconnected (Reconnecting...)`"
    };

    let bound_str = match bound_channel {
        Some(cid) => format!("<#{cid}>"),
        None => "*None (use `/syncplay bind`)*".to_string(),
    };

    let rooms_str = if status.active_rooms.is_empty() {
        format!("`#{}`", status.active_room)
    } else {
        status
            .active_rooms
            .iter()
            .map(|r| format!("`#{r}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let users_str = if status.active_users.is_empty() {
        "*No active users detected*".to_string()
    } else {
        status.active_users.join(", ")
    };

    let file_str = status.current_file.unwrap_or_else(|| "*No file loaded*".to_string());
    let play_str = if status.is_paused { "⏸️ Paused" } else { "▶️ Playing" };

    let embed = serenity::CreateEmbed::new()
        .title("Syncplay Bridge Status")
        .color(if status.is_connected { COLOR_SYNCPLAY } else { 0xE74C3C })
        .field("Connection", conn_str, true)
        .field("Server Host", format!("`{}`", status.host), true)
        .field("Primary Room", format!("`#{}`", status.active_room), true)
        .field("Bound Discord Channel", bound_str, true)
        .field("Active Rooms", rooms_str, true)
        .field("Playback State", format!("{play_str} • `{file_str}`"), false)
        .field("Active Viewers", users_str, false)
        .footer(serenity::CreateEmbedFooter::new(
            "Use /syncplay bind to switch channels or /syncplay room to switch primary room",
        ));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Switch the active Syncplay room without changing the bound channel.
#[poise::command(slash_command)]
pub async fn room(
    ctx: Context<'_>,
    #[description = "New Syncplay room name"]
    name: String,
    #[description = "Optional room password"]
    password: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let room_clean = name.trim();
    ctx.data()
        .syncplay
        .switch_room(room_clean, password.as_deref())
        .await?;

    let embed = serenity::CreateEmbed::new()
        .title("Syncplay Room Switched")
        .description(format!("Switched active Syncplay room to **`{room_clean}`**."))
        .color(COLOR_SYNCPLAY);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Send a message directly into the Syncplay room chat.
#[poise::command(slash_command)]
pub async fn send(
    ctx: Context<'_>,
    #[description = "Message to send into the Syncplay room"]
    message: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let author_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    ctx.data().syncplay.send_discord_chat(author_name, &message).await?;

    let embed = serenity::CreateEmbed::new()
        .description(format!("Sent to Syncplay room: *\"{message}\"*"))
        .color(0x2ECC71);

    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true)).await?;
    Ok(())
}

