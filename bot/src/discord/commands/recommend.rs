use poise::serenity_prelude as serenity;
use crate::anilist;
use crate::discord::commands::anime::autocomplete_anime;
use crate::discord::commands::anime_ui::{self, COLOR_RECOMMEND};
use crate::discord::{Context, Error};

/// Recommend anime to friends and track recommendations you've received.
#[poise::command(
    slash_command,
    subcommands("send", "list", "sent"),
    subcommand_required
)]
pub async fn recommend(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Recommend an anime to another user with an optional personal note.
#[poise::command(slash_command)]
pub async fn send(
    ctx: Context<'_>,
    #[description = "User you are recommending this anime to"]
    user: serenity::User,
    #[description = "Anime title to recommend"]
    #[autocomplete = "autocomplete_anime"]
    anime: String,
    #[description = "Optional note or reason why you recommend it"]
    note: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let sender = ctx.author();
    let sender_name = sender.global_name.as_deref().unwrap_or(&sender.name);
    let sender_avatar = anime_ui::user_avatar_url(sender);

    if user.id == sender.id {
        ctx.send(
            poise::CreateReply::default()
                .content("❌ You cannot recommend an anime to yourself! Use `/watchlist add` instead.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let http = &ctx.data().http;
    let db = &ctx.data().db;

    let media = if let Ok(id) = anime.trim().parse::<i32>() {
        anilist::get_anime_by_id(http, id).await.map_err(|e| e.to_string())?
    } else {
        anilist::get_anime_by_title(http, anime.trim()).await.map_err(|e| e.to_string())?
    };

    let target_anime = match media {
        Some(a) => a,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("❌ Could not find anime `{anime}` on AniList."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Cache anime in DB
    db.upsert_anime_cache(&target_anime).await?;

    // Create recommendation record in DB
    let rec_id = db
        .create_recommendation(
            sender.id.get(),
            user.id.get(),
            target_anime.id,
            note.as_deref(),
        )
        .await?;

    let site_url = target_anime.site_url.as_deref().unwrap_or("https://anilist.co");

    // Compose modern recommendation description
    let mut desc = String::new();
    if let Some(ref note_text) = note {
        desc.push_str(&format!("> 💬 *\"{note_text}\"*\n\n"));
    }

    let raw_desc = target_anime.clean_description();
    let (truncated, was_cut) = anime_ui::truncate_synopsis(&raw_desc, 300);
    desc.push_str(&truncated);
    if was_cut {
        desc.push_str(&format!("... [Read more]({site_url})"));
    }

    let format_str = anime_ui::clean_format(target_anime.format.as_deref());
    let status_str = anime_ui::clean_airing_status(target_anime.status.as_deref());

    let mut embed = serenity::CreateEmbed::new()
        .author(
            serenity::CreateEmbedAuthor::new(format!("Recommended by {sender_name}"))
                .icon_url(sender_avatar),
        )
        .title(target_anime.display_title())
        .url(site_url)
        .description(desc)
        .color(COLOR_RECOMMEND);

    // Cover thumbnail and banner
    if let Some(ref cover) = target_anime.cover_image {
        if let Some(img_url) = cover.best_url() {
            embed = embed.thumbnail(img_url);
        }
    }

    if let Some(ref banner) = target_anime.banner_image {
        embed = embed.image(banner);
    }

    // Metadata fields
    let score_str = target_anime
        .average_score
        .map(|s| format!("⭐ **{s}%**"))
        .unwrap_or_else(|| "—".to_string());
    let ep_str = target_anime
        .episodes
        .map(|ep| format!("**{ep}** eps"))
        .unwrap_or_else(|| "Ongoing".to_string());

    embed = embed.field("Rating", score_str, true);
    embed = embed.field("Format", format!("{format_str} • {ep_str} • {status_str}"), true);

    if !target_anime.genres.is_empty() {
        embed = embed.field("Genres", target_anime.genres.join(" • "), false);
    }

    embed = embed.footer(serenity::CreateEmbedFooter::new(
        "Click below to add this recommendation to your watchlist",
    ));

    // Interactive Button for recipient
    let button_id = format!("anime:rec_add:{}:{}:{}", rec_id, user.id.get(), target_anime.id);
    let add_button = serenity::CreateButton::new(button_id)
        .label("Add to Watchlist")
        .style(serenity::ButtonStyle::Primary);

    let action_row = serenity::CreateActionRow::Buttons(vec![add_button]);
    let message_text = format!("<@{}>, **{}** recommended an anime for you!", user.id, sender_name);

    ctx.send(
        poise::CreateReply::default()
            .content(message_text)
            .embed(embed)
            .components(vec![action_row]),
    )
    .await?;

    Ok(())
}

/// View anime recommendations you have received from other users.
#[poise::command(slash_command)]
pub async fn list(
    ctx: Context<'_>,
    #[description = "Filter recommendations received from a specific user"]
    from: Option<serenity::User>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let db = &ctx.data().db;
    let recipient_id = ctx.author().id.get();
    let sender_filter = from.as_ref().map(|u| u.id.get());

    let recs = db.get_received_recommendations(recipient_id, sender_filter).await?;

    let caller = ctx.author();
    let caller_name = caller.global_name.as_deref().unwrap_or(&caller.name);
    let caller_avatar = anime_ui::user_avatar_url(caller);

    if recs.is_empty() {
        let msg = match from {
            Some(u) => format!("You haven't received any recommendations from **{}**.", u.name),
            None => "You haven't received any anime recommendations yet!\nFriends can send you one with `/recommend send`.".to_string(),
        };
        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{caller_name}'s Recommendations")).icon_url(caller_avatar))
            .description(msg)
            .color(COLOR_RECOMMEND);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    let title = match from {
        Some(ref u) => format!("Recommendations from {}", u.name),
        None => "Received Recommendations".to_string(),
    };

    let mut description = String::new();
    let total = recs.len();

    for (idx, rec) in recs.iter().take(12).enumerate() {
        let status_badge = if rec.status == "added" {
            "✅ `Added to Watchlist`"
        } else {
            "⏳ `Pending`"
        };
        let site_url = rec.site_url.as_deref().unwrap_or("https://anilist.co");

        let title_line = format!("`{:2}` [**{}**]({})\n", idx + 1, rec.display_title(), site_url);
        let meta_line = format!("      From <@{}> • {}\n", rec.sender_id, status_badge);

        description.push_str(&title_line);
        description.push_str(&meta_line);

        if let Some(ref note) = rec.note {
            description.push_str(&format!("      > 💬 *\"{}\"*\n", note));
        }
        description.push('\n');
    }

    if total > 12 {
        description.push_str(&format!("*...and {} more recommendations.*", total - 12));
    }

    let mut embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new(format!("{caller_name}'s Recommendations")).icon_url(caller_avatar))
        .title(title)
        .description(description)
        .color(COLOR_RECOMMEND);

    if let Some(cover) = recs.first().and_then(|r| r.cover_url.as_deref()) {
        embed = embed.thumbnail(cover);
    }

    embed = embed.footer(serenity::CreateEmbedFooter::new(format!(
        "Total: {total} • Click [Add to Watchlist] on the recommendation card to accept"
    )));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// View anime recommendations you have sent to other users.
#[poise::command(slash_command)]
pub async fn sent(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let db = &ctx.data().db;
    let sender_id = ctx.author().id.get();

    let recs = db.get_sent_recommendations(sender_id).await?;

    let caller = ctx.author();
    let caller_name = caller.global_name.as_deref().unwrap_or(&caller.name);
    let caller_avatar = anime_ui::user_avatar_url(caller);

    if recs.is_empty() {
        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{caller_name}'s Sent Recommendations")).icon_url(caller_avatar))
            .description("You haven't sent any recommendations yet!\nUse `/recommend send` to recommend anime to a friend.")
            .color(0xF39C12);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    let mut description = String::new();
    let total = recs.len();

    for (idx, rec) in recs.iter().take(12).enumerate() {
        let status_badge = if rec.status == "added" {
            "✅ `Added`"
        } else {
            "⏳ `Pending`"
        };
        let site_url = rec.site_url.as_deref().unwrap_or("https://anilist.co");

        let title_line = format!("`{:2}` [**{}**]({})\n", idx + 1, rec.display_title(), site_url);
        let meta_line = format!("      Sent to <@{}> • {}\n", rec.recipient_id, status_badge);

        description.push_str(&title_line);
        description.push_str(&meta_line);

        if let Some(ref note) = rec.note {
            description.push_str(&format!("      > 💬 *\"{}\"*\n", note));
        }
        description.push('\n');
    }

    if total > 12 {
        description.push_str(&format!("*...and {} more recommendations.*", total - 12));
    }

    let mut embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new(format!("{caller_name}'s Recommendations")).icon_url(caller_avatar))
        .title("Sent Anime Recommendations")
        .description(description)
        .color(0xF39C12);

    if let Some(cover) = recs.first().and_then(|r| r.cover_url.as_deref()) {
        embed = embed.thumbnail(cover);
    }

    embed = embed.footer(serenity::CreateEmbedFooter::new(format!(
        "Total Sent: {total}"
    )));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
