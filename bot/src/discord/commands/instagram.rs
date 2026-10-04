use poise::serenity_prelude as serenity;
use crate::discord::{Context, Error};
use crate::instagram::webhook::{INSTAGRAM_ICON_URL, INSTAGRAM_MAGENTA};

/// Manage Instagram reel webhook configuration, target channels, and sender links.
#[poise::command(
    slash_command,
    subcommands("channel", "link", "senders"),
    subcommand_required
)]
pub async fn instagram(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Manage the target Discord channel where Instagram reels are posted.
#[poise::command(
    slash_command,
    subcommands("channel_set", "channel_get"),
    subcommand_required
)]
pub async fn channel(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Set the Discord channel where incoming Instagram reels should be posted.
#[poise::command(slash_command, rename = "set")]
pub async fn channel_set(
    ctx: Context<'_>,
    #[description = "Channel where reels will be posted"]
    target: serenity::Channel,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let db = &ctx.data().db;
    let channel_id_str = target.id().to_string();

    db.set_setting("instagram_target_channel_id", &channel_id_str).await?;

    let embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new("Instagram Integration").icon_url(INSTAGRAM_ICON_URL))
        .title("Target Channel Updated")
        .description(format!(
            "✅ Incoming Instagram reels will now be posted to <#{}>!",
            target.id()
        ))
        .color(INSTAGRAM_MAGENTA);

    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true)).await?;
    Ok(())
}

/// View the currently configured channel for Instagram reels.
#[poise::command(slash_command, rename = "get")]
pub async fn channel_get(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let db = &ctx.data().db;
    let setting = db.get_setting("instagram_target_channel_id").await?;

    let env_fallback = std::env::var("INSTAGRAM_TARGET_CHANNEL_ID")
        .or_else(|_| std::env::var("TARGET_CHANNEL_ID"))
        .ok();

    let desc = match (setting, env_fallback) {
        (Some(id), _) => format!("Active Target Channel: <#{id}> (Configured in Database)"),
        (None, Some(id)) => format!("Active Target Channel: <#{id}> (Configured in .env)"),
        (None, None) => "⚠️ No target channel configured yet. Use `/instagram channel set #channel` or set `INSTAGRAM_TARGET_CHANNEL_ID` in `.env`.".to_string(),
    };

    let embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new("Instagram Integration").icon_url(INSTAGRAM_ICON_URL))
        .title("Instagram Reel Destination")
        .description(desc)
        .color(INSTAGRAM_MAGENTA);

    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true)).await?;
    Ok(())
}

/// Link or update an Instagram numeric sender ID to a username and optional Discord user.
#[poise::command(slash_command)]
pub async fn link(
    ctx: Context<'_>,
    #[description = "Meta numeric sender ID (e.g. 1177907484531522)"]
    sender_id: String,
    #[description = "Instagram username without @ (e.g. Zarif_1020)"]
    username: String,
    #[description = "Optional Discord user to tag when this sender shares a reel"]
    discord_user: Option<serenity::User>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let clean_sender_id = sender_id.trim();
    let clean_username = username.trim().trim_start_matches('@');
    let discord_id = discord_user.as_ref().map(|u| u.id.get() as i64);

    let db = &ctx.data().db;
    db.upsert_instagram_sender(clean_sender_id, clean_username, discord_id).await?;

    let mut desc = format!(
        "Linked sender ID `{clean_sender_id}` to **@{clean_username}**!"
    );
    if let Some(user) = discord_user {
        desc.push_str(&format!("\n• Linked Discord Profile: <@{}>", user.id));
    }

    let embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new("Instagram Integration").icon_url(INSTAGRAM_ICON_URL))
        .title("Sender Profile Linked")
        .description(desc)
        .color(INSTAGRAM_MAGENTA);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// List all mapped Instagram senders.
#[poise::command(slash_command)]
pub async fn senders(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let db = &ctx.data().db;
    let list = db.list_instagram_senders().await?;

    if list.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .content("No Instagram senders mapped yet. Use `/instagram link <sender_id> <username>`!")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let mut desc = String::new();
    for (i, sender) in list.iter().enumerate() {
        let discord_str = sender
            .discord_user_id
            .map(|id| format!(" ➔ <@{id}>"))
            .unwrap_or_default();
        desc.push_str(&format!(
            "`{:2}.` **@{}** (`{}`){}\n",
            i + 1,
            sender.username,
            sender.sender_id,
            discord_str
        ));
    }

    let embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new("Instagram Integration").icon_url(INSTAGRAM_ICON_URL))
        .title(format!("Registered Instagram Senders ({})", list.len()))
        .description(desc)
        .color(INSTAGRAM_MAGENTA)
        .footer(serenity::CreateEmbedFooter::new(
            "Use /instagram link to map new sender IDs",
        ));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
