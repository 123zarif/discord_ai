use poise::serenity_prelude as serenity;
use crate::cinemeta::{self, CinemetaMedia};
use crate::discord::commands::anime_ui;
use crate::discord::{Context, Error};

const COLOR_MOVIELIST: u32 = 0xE50914; // Cinema Red

#[derive(Debug, poise::ChoiceParameter, Clone, Copy, PartialEq, Eq)]
pub enum MediaWatchStatus {
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

impl MediaWatchStatus {
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
            Self::Watching => "Watching",
            Self::Planning => "Plan to Watch",
            Self::Completed => "Completed",
            Self::OnHold => "On Hold",
            Self::Dropped => "Dropped",
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            Self::Watching => "`📺 Watching`",
            Self::Planning => "`📋 Plan to Watch`",
            Self::Completed => "`✅ Completed`",
            Self::OnHold => "`⏸️ On Hold`",
            Self::Dropped => "`❌ Dropped`",
        }
    }

    pub fn color(&self) -> u32 {
        match self {
            Self::Watching => 0x3498DB,
            Self::Planning => 0x9B59B6,
            Self::Completed => 0x2ECC71,
            Self::OnHold => 0xE67E22,
            Self::Dropped => 0xE74C3C,
        }
    }
}

#[derive(Debug, poise::ChoiceParameter, Clone, Copy, PartialEq, Eq)]
pub enum MediaTypeFilter {
    #[name = "Movies"]
    Movie,
    #[name = "TV Series"]
    Series,
}

impl MediaTypeFilter {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Series => "series",
        }
    }
}

/// Autocomplete provider for movies and TV series querying Cinemeta in parallel.
pub async fn autocomplete_media(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let clean = partial.trim();
    if clean.is_empty() {
        return serenity::CreateAutocompleteResponse::new().set_choices(vec![]);
    }

    let http = &ctx.data().http;
    let (movies_res, series_res) = tokio::join!(
        cinemeta::search_movies(http, clean, 5),
        cinemeta::search_series(http, clean, 5)
    );

    let mut choices = Vec::new();

    if let Ok(movies) = movies_res {
        for m in movies {
            let label = format!("[Movie] {}", m.display_label());
            choices.push(serenity::AutocompleteChoice::new(label, m.id));
        }
    }

    if let Ok(series) = series_res {
        for s in series {
            let label = format!("[TV] {}", s.display_label());
            choices.push(serenity::AutocompleteChoice::new(label, s.id));
        }
    }

    serenity::CreateAutocompleteResponse::new().set_choices(choices)
}

/// Fetches media from Cinemeta either by IMDb ID or query string.
async fn resolve_media(http: &reqwest::Client, input: &str) -> Result<Option<CinemetaMedia>, Error> {
    let query = input.trim();
    if query.starts_with("tt") && query.chars().skip(2).all(|c| c.is_ascii_digit()) {
        // Direct IMDb ID
        if let Some(m) = cinemeta::get_movie_by_id(http, query).await? {
            return Ok(Some(m));
        }
        if let Some(s) = cinemeta::get_series_by_id(http, query).await? {
            return Ok(Some(s));
        }
        Ok(None)
    } else {
        // Query search
        let (movies, series) = tokio::join!(
            cinemeta::search_movies(http, query, 1),
            cinemeta::search_series(http, query, 1)
        );

        if let Ok(m_list) = movies {
            if let Some(first) = m_list.into_iter().next() {
                return Ok(cinemeta::get_movie_by_id(http, &first.id).await?);
            }
        }
        if let Ok(s_list) = series {
            if let Some(first) = s_list.into_iter().next() {
                return Ok(cinemeta::get_series_by_id(http, &first.id).await?);
            }
        }
        Ok(None)
    }
}

/// Manage your personal movie and TV series watchlist.
#[poise::command(
    slash_command,
    subcommands("add", "view", "update", "remove"),
    subcommand_required
)]
pub async fn movielist(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add a movie or TV series to your watchlist.
#[poise::command(slash_command)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "Movie or TV show title to add"]
    #[autocomplete = "autocomplete_media"]
    media: String,
    #[description = "Watchlist status (default: Plan to Watch)"]
    status: Option<MediaWatchStatus>,
    #[description = "Current episode progress or minutes watched"]
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

    let resolved = resolve_media(http, &media).await?;
    let target = match resolved {
        Some(m) => m,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("Could not find movie or TV series `{media}`."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Cache in DB
    db.upsert_media_cache(&target).await?;

    let watch_status = status.unwrap_or(MediaWatchStatus::Planning);
    db.upsert_media_watchlist_entry(
        user_id,
        &target.id(),
        watch_status.as_db_str(),
        progress,
        score,
    )
    .await?;

    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());
    let color = watch_status.color();
    let imdb_link = format!("https://www.imdb.com/title/{}/", target.id());
    let media_type_label = if target.media_type == "series" { "TV Series" } else { "Movie" };

    let mut desc = format!("### [**{}**]({})\n\n", target.name, imdb_link);
    desc.push_str(&format!("**Type:** {}\n", media_type_label));
    desc.push_str(&format!("**Status:** {}\n", watch_status.badge()));

    if let Some(prog) = progress {
        if target.media_type == "series" {
            desc.push_str(&format!("**Progress:** Episode **{prog}**\n"));
        } else {
            desc.push_str(&format!("**Progress:** **{prog}** minutes\n"));
        }
    }

    if let Some(s) = score {
        desc.push_str(&format!("**Rating:** ⭐ **{s}**/10\n"));
    }

    let mut embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Movie & TV Watchlist")).icon_url(avatar))
        .title("Added to Watchlist")
        .description(desc)
        .color(color)
        .footer(serenity::CreateEmbedFooter::new(
            "Use /movielist update to log progress or change status",
        ));

    if let Some(ref poster) = target.poster {
        embed = embed.thumbnail(poster);
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// View your or another user's movie and TV series watchlist.
#[poise::command(slash_command)]
pub async fn view(
    ctx: Context<'_>,
    #[description = "User whose watchlist you want to view (defaults to you)"]
    user: Option<serenity::User>,
    #[description = "Filter by status (Watching, Completed, etc.)"]
    status: Option<MediaWatchStatus>,
    #[description = "Filter by media type (Movies or TV Series)"]
    media_type: Option<MediaTypeFilter>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let user_name = target_user.global_name.as_deref().unwrap_or(&target_user.name);
    let avatar = anime_ui::user_avatar_url(target_user);
    let db = &ctx.data().db;

    let type_filter = media_type.map(|t| t.as_db_str());
    let status_filter = status.map(|s| s.as_db_str());

    let entries = db
        .get_user_media_watchlist(target_user.id.get(), type_filter, status_filter)
        .await?;

    if entries.is_empty() {
        let msg = match (status, media_type) {
            (Some(s), Some(t)) => format!("No {} listed under **{}**.", match t {
                MediaTypeFilter::Movie => "movies",
                MediaTypeFilter::Series => "TV series",
            }, s.display_label()),
            (Some(s), None) => format!("No titles listed under **{}**.", s.display_label()),
            (None, Some(t)) => format!("No {} on this watchlist.", match t {
                MediaTypeFilter::Movie => "movies",
                MediaTypeFilter::Series => "TV series",
            }),
            (None, None) => "Movie & TV watchlist is currently empty.\nUse `/movielist add <title>` or `/movie <title>` / `/tv <title>` to add titles!".to_string(),
        };

        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
            .description(msg)
            .color(COLOR_MOVIELIST);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    let title = match (status, media_type) {
        (Some(s), Some(t)) => format!("{} — {} ({})", user_name, s.display_label(), match t {
            MediaTypeFilter::Movie => "Movies",
            MediaTypeFilter::Series => "TV Series",
        }),
        (Some(s), None) => format!("{} — {}", user_name, s.display_label()),
        (None, Some(t)) => format!("{}'s {}", user_name, match t {
            MediaTypeFilter::Movie => "Movie Watchlist",
            MediaTypeFilter::Series => "TV Series Watchlist",
        }),
        (None, None) => format!("{user_name}'s Movie & TV Watchlist"),
    };

    let embed_color = match status {
        Some(s) => s.color(),
        None => COLOR_MOVIELIST,
    };

    let total_count = entries.len();
    let mut description = String::new();

    // Summary banner if viewing all
    if status.is_none() && media_type.is_none() {
        let mut movies_cnt = 0;
        let mut series_cnt = 0;
        let mut completed_cnt = 0;
        let mut watching_cnt = 0;

        for e in &entries {
            if e.media_type == "series" {
                series_cnt += 1;
            } else {
                movies_cnt += 1;
            }
            if e.status == "completed" {
                completed_cnt += 1;
            } else if e.status == "watching" {
                watching_cnt += 1;
            }
        }

        let mut pills = Vec::new();
        if movies_cnt > 0 { pills.push(format!("🎬 **{movies_cnt}** Movies")); }
        if series_cnt > 0 { pills.push(format!("📺 **{series_cnt}** Series")); }
        if watching_cnt > 0 { pills.push(format!("▶️ **{watching_cnt}** Watching")); }
        if completed_cnt > 0 { pills.push(format!("✅ **{completed_cnt}** Completed")); }

        if !pills.is_empty() {
            description.push_str(&pills.join("  •  "));
            description.push_str("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n");
        }
    }

    let display_entries = entries.iter().take(15);
    for (idx, entry) in display_entries.enumerate() {
        let status_enum = MediaWatchStatus::from_db_str(&entry.status);
        let imdb_link = format!("https://www.imdb.com/title/{}/", entry.imdb_id);
        let type_tag = if entry.media_type == "series" { "TV" } else { "Movie" };
        let year_str = entry.year.as_deref().map(|y| format!(" ({y})")).unwrap_or_default();

        let title_line = format!("`{:2}` `[{}]` [**{}{}**]({})\n", idx + 1, type_tag, entry.title, year_str, imdb_link);

        let mut details = Vec::new();
        details.push(status_enum.display_label().to_string());

        if entry.progress > 0 {
            if entry.media_type == "series" {
                details.push(format!("Ep **{}**", entry.progress));
            } else {
                details.push(format!("**{}**m", entry.progress));
            }
        }

        if let Some(score) = entry.score {
            details.push(format!("⭐ **{score}**/10"));
        }

        if let Some(ref rating) = entry.imdb_rating {
            details.push(format!("IMDb **{rating}**"));
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

    if let Some(poster) = entries.first().and_then(|e| e.poster_url.as_deref()) {
        embed = embed.thumbnail(poster);
    }

    let footer_text = if total_count > 15 {
        format!("Showing 1-15 of {total_count} entries • Use /movielist update to log progress")
    } else {
        format!("{total_count} total entries • Use /movielist update to log progress")
    };
    embed = embed.footer(serenity::CreateEmbedFooter::new(footer_text));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Update progress, rating, or status of a movie or TV show on your watchlist.
#[poise::command(slash_command)]
pub async fn update(
    ctx: Context<'_>,
    #[description = "Movie or TV show title to update"]
    #[autocomplete = "autocomplete_media"]
    media: String,
    #[description = "New watchlist status"]
    status: Option<MediaWatchStatus>,
    #[description = "Episode or minute progress"]
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

    let resolved = resolve_media(http, &media).await?;
    let target = match resolved {
        Some(m) => m,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("Could not find movie or TV series `{media}`."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    let new_status = status.unwrap_or(MediaWatchStatus::Watching);
    db.upsert_media_watchlist_entry(
        user_id,
        &target.id(),
        new_status.as_db_str(),
        progress,
        score,
    )
    .await?;

    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());
    let color = new_status.color();
    let imdb_link = format!("https://www.imdb.com/title/{}/", target.id());

    let mut desc = format!("### [**{}**]({})\n\n", target.name, imdb_link);
    desc.push_str(&format!("**Status:** {}\n", new_status.badge()));

    if let Some(prog) = progress {
        if target.media_type == "series" {
            desc.push_str(&format!("**Progress:** Episode **{prog}**\n"));
        } else {
            desc.push_str(&format!("**Progress:** **{prog}** minutes\n"));
        }
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
            "Updated successfully • Use /movielist view to view your list",
        ));

    if let Some(ref poster) = target.poster {
        embed = embed.thumbnail(poster);
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Remove a movie or TV show from your watchlist.
#[poise::command(slash_command)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Movie or TV show title to remove"]
    #[autocomplete = "autocomplete_media"]
    media: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let http = &ctx.data().http;
    let db = &ctx.data().db;
    let user_id = ctx.author().id.get();

    let resolved = resolve_media(http, &media).await?;
    let target = match resolved {
        Some(m) => m,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("Could not find movie or TV series `{media}`."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());
    let imdb_link = format!("https://www.imdb.com/title/{}/", target.id());

    let removed = db.remove_from_media_watchlist(user_id, &target.id()).await?;
    if removed {
        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
            .title("Removed from Watchlist")
            .description(format!(
                "Removed [**{}**]({}) from your watchlist.",
                target.name, imdb_link
            ))
            .color(0x95A5A6);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    } else {
        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
            .description(format!(
                "[**{}**]({}) was not on your watchlist.",
                target.name, imdb_link
            ))
            .color(0xE67E22);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    }

    Ok(())
}

