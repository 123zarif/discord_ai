use poise::serenity_prelude as serenity;
use crate::anilist;
use crate::discord::commands::anime::autocomplete_anime;
use crate::discord::commands::anime_ui;
use crate::discord::{Context, Error};

#[derive(Debug, poise::ChoiceParameter, Clone, Copy, PartialEq, Eq)]
pub enum WatchStatus {
    #[name = "Watching"]
    Watching,
    #[name = "Plan to Watch"]
    Planning,
    #[name = "Completed"]
    Completed,
    #[name = "On Hold"]
    OnHold,
    #[name = "Dropped"]
    Dropped,
}

impl WatchStatus {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Watching => "watching",
            Self::Planning => "planning",
            Self::Completed => "completed",
            Self::OnHold => "on_hold",
            Self::Dropped => "dropped",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "watching" => Self::Watching,
            "completed" => Self::Completed,
            "on_hold" => Self::OnHold,
            "dropped" => Self::Dropped,
            _ => Self::Planning,
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Watching => "📺 Watching",
            Self::Planning => "📋 Plan to Watch",
            Self::Completed => "✅ Completed",
            Self::OnHold => "⏸️ On Hold",
            Self::Dropped => "❌ Dropped",
        }
    }
}

/// Manage your personal anime watchlist and episode progress.
#[poise::command(
    slash_command,
    subcommands("add", "view", "update", "remove"),
    subcommand_required
)]
pub async fn watchlist(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add an anime to your watchlist.
#[poise::command(slash_command)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "Anime title to add"]
    #[autocomplete = "autocomplete_anime"]
    anime: String,
    #[description = "Watchlist status (default: Plan to Watch)"]
    status: Option<WatchStatus>,
    #[description = "Current episode progress (e.g. 5)"]
    progress: Option<i32>,
    #[description = "Your score (1 to 10)"]
    #[min = 1]
    #[max = 10]
    score: Option<i32>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let http = &ctx.data().http;
    let db = &ctx.data().db;
    let user_id = ctx.author().id.get();

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

    let watch_status = status.unwrap_or(WatchStatus::Planning);
    db.upsert_watchlist_entry(
        user_id,
        target_anime.id,
        watch_status.as_db_str(),
        progress,
        score,
    )
    .await?;

    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());
    let color = anime_ui::status_color(watch_status.as_db_str());
    let site_url = target_anime.site_url.as_deref().unwrap_or("https://anilist.co");

    let mut desc = format!("### [**{}**]({})\n\n", target_anime.display_title(), site_url);
    desc.push_str(&format!("**Status:** {}\n", anime_ui::status_badge(watch_status.as_db_str())));

    if let Some(prog) = progress {
        let p_bar = anime_ui::make_progress_bar(prog, target_anime.episodes, 8);
        if let Some(total) = target_anime.episodes {
            if !p_bar.is_empty() {
                desc.push_str(&format!("**Progress:** {p_bar} **{prog}** / {total} eps\n"));
            } else {
                desc.push_str(&format!("**Progress:** **{prog}** / {total} eps\n"));
            }
        } else {
            desc.push_str(&format!("**Progress:** Episode **{prog}**\n"));
        }
    } else if let Some(total) = target_anime.episodes {
        desc.push_str(&format!("**Episodes:** {total} total\n"));
    }

    if let Some(s) = score {
        desc.push_str(&format!("**Rating:** ⭐ **{s}**/10\n"));
    }

    let mut embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
        .title("Added to Watchlist")
        .description(desc)
        .color(color)
        .footer(serenity::CreateEmbedFooter::new(
            "Use /watchlist update to log new episodes or change status",
        ));

    if let Some(ref cover) = target_anime.cover_image {
        if let Some(img_url) = cover.best_url() {
            embed = embed.thumbnail(img_url);
        }
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// View your or another user's anime watchlist.
#[poise::command(slash_command)]
pub async fn view(
    ctx: Context<'_>,
    #[description = "User whose watchlist you want to view (defaults to you)"]
    user: Option<serenity::User>,
    #[description = "Filter by status (Watching, Completed, etc.)"]
    status: Option<WatchStatus>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let user_name = target_user.global_name.as_deref().unwrap_or(&target_user.name);
    let avatar = anime_ui::user_avatar_url(target_user);
    let db = &ctx.data().db;

    let filter_str = status.map(|s| s.as_db_str());
    let entries = db.get_user_watchlist(target_user.id.get(), filter_str).await?;

    if entries.is_empty() {
        let msg = match status {
            Some(s) => format!("No anime listed under **{}**.", s.display_label()),
            None => "Watchlist is currently empty.\nUse `/watchlist add <title>` or `/anime <title>` to add shows!".to_string(),
        };
        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
            .description(msg)
            .color(0x5865F2);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    let title = match status {
        Some(s) => format!("{} — {}", user_name, s.display_label()),
        None => format!("{user_name}'s Anime Watchlist"),
    };

    let embed_color = match status {
        Some(s) => anime_ui::status_color(s.as_db_str()),
        None => anime_ui::COLOR_ANILIST,
    };

    let total_count = entries.len();
    let mut description = String::new();

    // If viewing all, show a neat summary banner at the top
    if status.is_none() {
        let mut watching_cnt = 0;
        let mut completed_cnt = 0;
        let mut planning_cnt = 0;
        let mut on_hold_cnt = 0;
        let mut dropped_cnt = 0;

        for e in &entries {
            match e.status.as_str() {
                "watching" => watching_cnt += 1,
                "completed" => completed_cnt += 1,
                "planning" => planning_cnt += 1,
                "on_hold" => on_hold_cnt += 1,
                "dropped" => dropped_cnt += 1,
                _ => {}
            }
        }

        let mut pills = Vec::new();
        if watching_cnt > 0 { pills.push(format!("📺 **{watching_cnt}** Watching")); }
        if completed_cnt > 0 { pills.push(format!("✅ **{completed_cnt}** Completed")); }
        if planning_cnt > 0 { pills.push(format!("📋 **{planning_cnt}** Planned")); }
        if on_hold_cnt > 0 { pills.push(format!("⏸️ **{on_hold_cnt}** On Hold")); }
        if dropped_cnt > 0 { pills.push(format!("🛑 **{dropped_cnt}** Dropped")); }

        if !pills.is_empty() {
            description.push_str(&pills.join("  •  "));
            description.push_str("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n");
        }
    }

    let display_entries = entries.iter().take(15);
    for (idx, entry) in display_entries.enumerate() {
        let status_enum = WatchStatus::from_db_str(&entry.status);
        let site_url = entry.site_url.as_deref().unwrap_or("https://anilist.co");

        let title_line = format!("`{:2}` [**{}**]({})\n", idx + 1, entry.display_title(), site_url);

        let mut details = Vec::new();
        details.push(status_enum.display_label().to_string());

        if entry.status == "completed" {
            if let Some(total) = entry.episodes {
                details.push(format!("**{total}/{total}** eps"));
            } else if entry.progress > 0 {
                details.push(format!("**{}** eps", entry.progress));
            }
        } else if entry.progress > 0 || entry.status == "watching" {
            let p_bar = anime_ui::make_progress_bar(entry.progress, entry.episodes, 6);
            if let Some(total) = entry.episodes {
                if !p_bar.is_empty() {
                    details.push(format!("{p_bar} **{}**/{}", entry.progress, total));
                } else {
                    details.push(format!("**{}/{}** eps", entry.progress, total));
                }
            } else if entry.progress > 0 {
                details.push(format!("Ep **{}**", entry.progress));
            }
        } else if let Some(total) = entry.episodes {
            details.push(format!("{total} eps"));
        }

        if let Some(score) = entry.score {
            details.push(format!("⭐ **{score}**/10"));
        }

        let detail_line = format!("      {}\n\n", details.join(" • "));
        description.push_str(&title_line);
        description.push_str(&detail_line);
    }

    if total_count > 15 {
        description.push_str(&format!("*...and {} more entries.*", total_count - 15));
    }

    let mut embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
        .title(title)
        .description(description)
        .color(embed_color);

    // If first entry has a cover image, use as thumbnail
    if let Some(cover) = entries.first().and_then(|e| e.cover_url.as_deref()) {
        embed = embed.thumbnail(cover);
    }

    let footer_text = if total_count > 15 {
        format!("Showing 1-15 of {total_count} entries • Use /watchlist update to log episodes")
    } else {
        format!("{total_count} total entries • Use /watchlist update to log episodes")
    };
    embed = embed.footer(serenity::CreateEmbedFooter::new(footer_text));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Update progress, rating, or status of an anime already on your watchlist.
#[poise::command(slash_command)]
pub async fn update(
    ctx: Context<'_>,
    #[description = "Anime title to update"]
    #[autocomplete = "autocomplete_anime"]
    anime: String,
    #[description = "New watchlist status"]
    status: Option<WatchStatus>,
    #[description = "Episode progress"]
    progress: Option<i32>,
    #[description = "Rating score (1 to 10)"]
    #[min = 1]
    #[max = 10]
    score: Option<i32>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let http = &ctx.data().http;
    let db = &ctx.data().db;
    let user_id = ctx.author().id.get();

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

    let new_status = status.unwrap_or(WatchStatus::Watching);
    db.upsert_watchlist_entry(
        user_id,
        target_anime.id,
        new_status.as_db_str(),
        progress,
        score,
    )
    .await?;

    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());
    let color = anime_ui::status_color(new_status.as_db_str());
    let site_url = target_anime.site_url.as_deref().unwrap_or("https://anilist.co");

    let mut desc = format!("### [**{}**]({})\n\n", target_anime.display_title(), site_url);
    desc.push_str(&format!("**Status:** {}\n", anime_ui::status_badge(new_status.as_db_str())));

    if let Some(prog) = progress {
        let p_bar = anime_ui::make_progress_bar(prog, target_anime.episodes, 8);
        if let Some(total) = target_anime.episodes {
            if !p_bar.is_empty() {
                desc.push_str(&format!("**Progress:** {p_bar} **{prog}** / {total} eps\n"));
            } else {
                desc.push_str(&format!("**Progress:** **{prog}** / {total} eps\n"));
            }
        } else {
            desc.push_str(&format!("**Progress:** Episode **{prog}**\n"));
        }
    } else if let Some(total) = target_anime.episodes {
        desc.push_str(&format!("**Episodes:** {total} total\n"));
    }

    if let Some(s) = score {
        desc.push_str(&format!("**Rating:** ⭐ **{s}**/10\n"));
    }

    let mut embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
        .title("Watchlist Updated")
        .description(desc)
        .color(color)
        .footer(serenity::CreateEmbedFooter::new(
            "Updated successfully • Use /watchlist view to see your list",
        ));

    if let Some(ref cover) = target_anime.cover_image {
        if let Some(img_url) = cover.best_url() {
            embed = embed.thumbnail(img_url);
        }
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Remove an anime from your watchlist.
#[poise::command(slash_command)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Anime title to remove"]
    #[autocomplete = "autocomplete_anime"]
    anime: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let http = &ctx.data().http;
    let db = &ctx.data().db;
    let user_id = ctx.author().id.get();

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

    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());
    let site_url = target_anime.site_url.as_deref().unwrap_or("https://anilist.co");

    let removed = db.remove_from_watchlist(user_id, target_anime.id).await?;
    if removed {
        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
            .title("Removed from Watchlist")
            .description(format!(
                "🗑️ Removed [**{}**]({}) from your watchlist.",
                target_anime.display_title(),
                site_url
            ))
            .color(anime_ui::COLOR_DROPPED);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    } else {
        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
            .description(format!(
                "⚠️ [**{}**]({}) was not on your watchlist.",
                target_anime.display_title(),
                site_url
            ))
            .color(anime_ui::COLOR_ON_HOLD);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    }

    Ok(())
}
