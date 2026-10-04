use poise::serenity_prelude as serenity;
use crate::discord::{Context, Error};

/// Manage Instagram webhook configuration, target channels, and sender links.
#[poise::command(
    slash_command,
    subcommands("channel", "link", "senders"),
    subcommand_required
)]
pub async fn instagram(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Manage the target Discord channel where Instagram media is posted.
#[poise::command(
    slash_command,
    subcommands("channel_set", "channel_get"),
    subcommand_required
)]
pub async fn channel(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Set the Discord channel where incoming Instagram media should be posted.
#[poise::command(slash_command, rename = "set")]
pub async fn channel_set(
    ctx: Context<'_>,
    #[description = "Channel where media will be posted"]
    target: serenity::Channel,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let db = &ctx.data().db;
    let channel_id_str = target.id().to_string();

    db.set_setting("instagram_target_channel_id", &channel_id_str).await?;

    ctx.send(
        poise::CreateReply::default()
            .content(format!("Target channel set to <#{}>.", target.id()))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// View the currently configured channel for Instagram media.
#[poise::command(slash_command, rename = "get")]
pub async fn channel_get(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let db = &ctx.data().db;
    let setting = db.get_setting("instagram_target_channel_id").await?;

    let env_fallback = std::env::var("INSTAGRAM_TARGET_CHANNEL_ID")
        .or_else(|_| std::env::var("TARGET_CHANNEL_ID"))
        .ok();

    let text = match (setting, env_fallback) {
        (Some(id), _) => format!("Target channel: <#{id}> (database)"),
        (None, Some(id)) => format!("Target channel: <#{id}> (.env)"),
        (None, None) => "No target channel configured. Set one with `/instagram channel set #channel`.".to_string(),
    };

    ctx.send(
        poise::CreateReply::default()
            .content(text)
            .ephemeral(true),
    )
    .await?;

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
    #[description = "Optional Discord user to tag when this sender shares media"]
    discord_user: Option<serenity::User>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let clean_sender_id = sender_id.trim();
    let clean_username = username.trim().trim_start_matches('@');
    let discord_id = discord_user.as_ref().map(|u| u.id.get() as i64);

    let db = &ctx.data().db;
    db.upsert_instagram_sender(clean_sender_id, clean_username, discord_id).await?;

    let mut response = format!("Linked `{clean_sender_id}` to **@{clean_username}**.");
    if let Some(user) = discord_user {
        response.push_str(&format!(" Discord account: <@{}>", user.id));
    }

    ctx.send(poise::CreateReply::default().content(response)).await?;
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
                .content("No senders mapped yet. Use `/instagram link <sender_id> <username>`.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let mut desc = String::new();
    for (i, sender) in list.iter().enumerate() {
        let discord_str = sender
            .discord_user_id
            .map(|id| format!(" -> <@{id}>"))
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
        .title(format!("Registered Senders ({})", list.len()))
        .description(desc)
        .color(0x5865F2);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
