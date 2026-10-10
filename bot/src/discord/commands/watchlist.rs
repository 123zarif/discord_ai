use poise::serenity_prelude as serenity;
use crate::anilist::{self, AnimeMedia};
use crate::cinemeta::{self, CinemetaMedia};
use crate::db::media::UnifiedWatchlistEntry;
use crate::discord::commands::anime_ui;
use crate::discord::commands::recommend::autocomplete_recommend_media;
use crate::discord::{Context, Error};

const COLOR_UNIFIED_WATCHLIST: u32 = 0x5865F2; // Blurple
pub const ITEMS_PER_PAGE: usize = 10;

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
pub enum WatchlistTypeFilter {
    #[name = "All Media"]
    All,
    #[name = "Anime"]
    Anime,
    #[name = "Movies"]
    Movie,
    #[name = "TV Series"]
    Series,
}

impl WatchlistTypeFilter {
    pub fn as_db_str(&self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Anime => Some("anime"),
            Self::Movie => Some("movie"),
            Self::Series => Some("series"),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "anime" => Self::Anime,
            "movie" => Self::Movie,
            "series" => Self::Series,
            _ => Self::All,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::All => "All Media",
            Self::Anime => "Anime",
            Self::Movie => "Movies",
            Self::Series => "TV Series",
        }
    }
}

enum ResolvedWatchlistMedia {
    Anime(AnimeMedia),
    Movie(CinemetaMedia),
    Series(CinemetaMedia),
}

/// Resolves user input into Anime, Movie, or TV Series.
async fn resolve_input_media(http: &reqwest::Client, input: &str) -> Result<Option<ResolvedWatchlistMedia>, Error> {
    let clean = input.trim();

    // Check autocomplete prefixes
    if let Some(id_str) = clean.strip_prefix("anime:") {
        if let Ok(id) = id_str.parse::<i32>() {
            return Ok(anilist::get_anime_by_id(http, id).await?.map(ResolvedWatchlistMedia::Anime));
        }
    } else if let Some(imdb_id) = clean.strip_prefix("movie:") {
        return Ok(cinemeta::get_movie_by_id(http, imdb_id).await?.map(ResolvedWatchlistMedia::Movie));
    } else if let Some(imdb_id) = clean.strip_prefix("series:") {
        return Ok(cinemeta::get_series_by_id(http, imdb_id).await?.map(ResolvedWatchlistMedia::Series));
    }

    // Direct IMDb ID
    if clean.starts_with("tt") && clean.chars().skip(2).all(|c| c.is_ascii_digit()) {
        if let Some(m) = cinemeta::get_movie_by_id(http, clean).await? {
            return Ok(Some(ResolvedWatchlistMedia::Movie(m)));
        }
        if let Some(s) = cinemeta::get_series_by_id(http, clean).await? {
            return Ok(Some(ResolvedWatchlistMedia::Series(s)));
        }
    }

    // Direct AniList numeric ID
    if let Ok(id) = clean.parse::<i32>() {
        if let Some(a) = anilist::get_anime_by_id(http, id).await? {
            return Ok(Some(ResolvedWatchlistMedia::Anime(a)));
        }
    }

    // Search AniList first, then Cinemeta
    if let Some(a) = anilist::get_anime_by_title(http, clean).await? {
        return Ok(Some(ResolvedWatchlistMedia::Anime(a)));
    }

    let m_res = cinemeta::search_movies(http, clean, 1).await.unwrap_or_default();
    if let Some(first) = m_res.into_iter().next() {
        return Ok(cinemeta::get_movie_by_id(http, &first.id).await?.map(ResolvedWatchlistMedia::Movie));
    }

    let s_res = cinemeta::search_series(http, clean, 1).await.unwrap_or_default();
    if let Some(first) = s_res.into_iter().next() {
        return Ok(cinemeta::get_series_by_id(http, &first.id).await?.map(ResolvedWatchlistMedia::Series));
    }

    Ok(None)
}

/// Autocomplete provider for items already on the user's watchlist with live search.
pub async fn autocomplete_user_watchlist(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let user_id = ctx.author().id.get();
    let entries = ctx
        .data()
        .db
        .get_unified_watchlist(user_id, None, None)
        .await
        .unwrap_or_default();

    let clean = partial.trim().to_lowercase();

    let matching_entries: Vec<&UnifiedWatchlistEntry> = if clean.is_empty() {
        entries.iter().take(25).collect()
    } else {
        entries
            .iter()
            .filter(|e| e.title.to_lowercase().contains(&clean))
            .take(25)
            .collect()
    };

    if !matching_entries.is_empty() {
        let choices: Vec<serenity::AutocompleteChoice> = matching_entries
            .into_iter()
            .map(|e| {
                let type_badge = match e.media_type.as_str() {
                    "anime" => "Anime",
                    "movie" => "Movie",
                    "series" => "TV",
                    _ => "Media",
                };

                let prog_or_status = if e.progress > 0 {
                    if let Some(total) = e.total_progress {
                        format!("Ep {}/{}", e.progress, total)
                    } else if e.media_type == "movie" {
                        format!("{}m", e.progress)
                    } else {
                        format!("Ep {}", e.progress)
                    }
                } else {
                    WatchStatus::from_db_str(&e.status).display_label().to_string()
                };

                let mut label = format!("[{type_badge}] {} ({prog_or_status})", e.title);
                if label.len() > 100 {
                    label.truncate(97);
                    label.push_str("...");
                }

                let value = format!("{}:{}", e.media_type, e.media_id);
                serenity::AutocompleteChoice::new(label, value)
            })
            .collect();

        return serenity::CreateAutocompleteResponse::new().set_choices(choices);
    }

    // Fallback to general media search if no match on own watchlist
    autocomplete_recommend_media(ctx, partial).await
}

/// Manage your unified watchlist across Anime, Movies, and TV series.
#[poise::command(
    slash_command,
    subcommands("add", "view", "update", "remove"),
    subcommand_required
)]
pub async fn watchlist(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add an anime, movie, or TV series to your watchlist.
#[poise::command(slash_command)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "Title to add (search anime, movies, and TV series)"]
    #[autocomplete = "autocomplete_recommend_media"]
    media: String,
    #[description = "Watchlist status (default: Plan to Watch)"]
    status: Option<WatchStatus>,
    #[description = "Episode or minute progress"]
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

    let target = match resolve_input_media(http, &media).await? {
        Some(m) => m,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("Could not find media matching `{media}`."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    let watch_status = status.unwrap_or(WatchStatus::Planning);
    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());
    let color = watch_status.color();

    match target {
        ResolvedWatchlistMedia::Anime(a) => {
            db.upsert_anime_cache(&a).await?;
            db.upsert_watchlist_entry(user_id, a.id, watch_status.as_db_str(), progress, score).await?;

            let site = a.site_url.as_deref().unwrap_or("https://anilist.co");
            let mut desc = format!("### [**{}**]({})\n\n", a.display_title(), site);
            desc.push_str("**Type:** `Anime`\n");
            desc.push_str(&format!("**Status:** {}\n", watch_status.badge()));

            if let Some(prog) = progress {
                let p_bar = anime_ui::make_progress_bar(prog, a.episodes, 8);
                if let Some(total) = a.episodes {
                    if !p_bar.is_empty() {
                        desc.push_str(&format!("**Progress:** {p_bar} **{prog}** / {total} eps\n"));
                    } else {
                        desc.push_str(&format!("**Progress:** **{prog}** / {total} eps\n"));
                    }
                } else {
                    desc.push_str(&format!("**Progress:** Episode **{prog}**\n"));
                }
            } else if let Some(total) = a.episodes {
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
                .footer(serenity::CreateEmbedFooter::new("Use /watchlist update to log progress or change status"));

            if let Some(ref cover) = a.cover_image {
                if let Some(img_url) = cover.best_url() {
                    embed = embed.thumbnail(img_url);
                }
            }

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
        ResolvedWatchlistMedia::Movie(m) => {
            db.upsert_media_cache(&m).await?;
            db.upsert_media_watchlist_entry(user_id, &m.id(), watch_status.as_db_str(), progress, score).await?;

            let imdb_link = format!("https://www.imdb.com/title/{}/", m.id());
            let mut desc = format!("### [**{}**]({})\n\n", m.name, imdb_link);
            desc.push_str("**Type:** `Movie`\n");
            desc.push_str(&format!("**Status:** {}\n", watch_status.badge()));

            if let Some(prog) = progress {
                desc.push_str(&format!("**Progress:** **{prog}** minutes\n"));
            } else if let Some(ref rt) = m.runtime {
                desc.push_str(&format!("**Runtime:** {rt}\n"));
            }

            if let Some(s) = score {
                desc.push_str(&format!("**Rating:** ⭐ **{s}**/10\n"));
            }

            let mut embed = serenity::CreateEmbed::new()
                .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                .title("Added to Watchlist")
                .description(desc)
                .color(color)
                .footer(serenity::CreateEmbedFooter::new("Use /watchlist update to log progress or change status"));

            if let Some(ref poster) = m.poster {
                embed = embed.thumbnail(poster);
            }

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
        ResolvedWatchlistMedia::Series(s) => {
            db.upsert_media_cache(&s).await?;
            db.upsert_media_watchlist_entry(user_id, &s.id(), watch_status.as_db_str(), progress, score).await?;

            let imdb_link = format!("https://www.imdb.com/title/{}/", s.id());
            let mut desc = format!("### [**{}**]({})\n\n", s.name, imdb_link);
            desc.push_str("**Type:** `TV Series`\n");
            desc.push_str(&format!("**Status:** {}\n", watch_status.badge()));

            if let Some(prog) = progress {
                desc.push_str(&format!("**Progress:** Episode **{prog}**\n"));
            }

            if let Some(s) = score {
                desc.push_str(&format!("**Rating:** ⭐ **{s}**/10\n"));
            }

            let mut embed = serenity::CreateEmbed::new()
                .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                .title("Added to Watchlist")
                .description(desc)
                .color(color)
                .footer(serenity::CreateEmbedFooter::new("Use /watchlist update to log progress or change status"));

            if let Some(ref poster) = s.poster {
                embed = embed.thumbnail(poster);
            }

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
    }

    Ok(())
}

/// View your or another user's unified watchlist with page navigation.
#[poise::command(slash_command)]
pub async fn view(
    ctx: Context<'_>,
    #[description = "User whose watchlist you want to view (defaults to you)"]
    user: Option<serenity::User>,
    #[description = "Filter by status (Watching, Completed, etc.)"]
    status: Option<WatchStatus>,
    #[description = "Filter by media type (Anime, Movies, TV Series)"]
    media_type: Option<WatchlistTypeFilter>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let caller_id = ctx.author().id.get();
    let db = &ctx.data().db;

    let type_filter_str = media_type.and_then(|t| t.as_db_str());
    let status_filter_str = status.map(|s| s.as_db_str());

    let entries = db
        .get_unified_watchlist(target_user.id.get(), type_filter_str, status_filter_str)
        .await?;

    let (embed, action_row) = render_watchlist_view(
        caller_id,
        target_user,
        &entries,
        1,
        status,
        media_type,
    );

    let mut reply = poise::CreateReply::default().embed(embed);
    if let Some(row) = action_row {
        reply = reply.components(vec![row]);
    }

    ctx.send(reply).await?;
    Ok(())
}

/// Update progress, rating, or status of an item on your watchlist.
#[poise::command(slash_command)]
pub async fn update(
    ctx: Context<'_>,
    #[description = "Title to update from your watchlist (leave empty to view your complete list first)"]
    #[autocomplete = "autocomplete_user_watchlist"]
    media: Option<String>,
    #[description = "New watchlist status"]
    status: Option<WatchStatus>,
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
    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);

    let media_input = match media {
        Some(m) if !m.trim().is_empty() => m,
        _ => {
            let entries = db.get_unified_watchlist(user_id, None, None).await?;
            if entries.is_empty() {
                ctx.send(
                    poise::CreateReply::default()
                        .content("Your watchlist is currently empty. Use `/watchlist add <title>` to add titles.")
                        .ephemeral(true),
                )
                .await?;
                return Ok(());
            }

            let (mut embed, action_row) = render_watchlist_view(
                user_id,
                ctx.author(),
                &entries,
                1,
                None,
                None,
            );
            embed = embed.title(format!("{user_name}'s Watchlist — Ready to Update"));
            embed = embed.footer(serenity::CreateEmbedFooter::new(
                "Pick a title with /watchlist update media:<title> (or choose from autocomplete)",
            ));

            let mut reply = poise::CreateReply::default().embed(embed);
            if let Some(row) = action_row {
                reply = reply.components(vec![row]);
            }
            ctx.send(reply).await?;
            return Ok(());
        }
    };

    let target = match resolve_input_media(http, &media_input).await? {
        Some(m) => m,
        None => {
            let entries = db.get_unified_watchlist(user_id, None, None).await.unwrap_or_default();
            let matched = entries.into_iter().find(|e| {
                e.title.eq_ignore_ascii_case(&media_input)
                    || e.media_id == media_input
                    || e.title.to_lowercase().contains(&media_input.to_lowercase())
            });

            if let Some(e) = matched {
                let formatted = format!("{}:{}", e.media_type, e.media_id);
                match resolve_input_media(http, &formatted).await? {
                    Some(m) => m,
                    None => {
                        ctx.send(
                            poise::CreateReply::default()
                                .content(format!("Could not resolve `{}` from your watchlist.", e.title))
                                .ephemeral(true),
                        )
                        .await?;
                        return Ok(());
                    }
                }
            } else {
                ctx.send(
                    poise::CreateReply::default()
                        .content(format!("Could not find `{media_input}` on your watchlist."))
                        .ephemeral(true),
                )
                .await?;
                return Ok(());
            }
        }
    };

    let new_status = status.unwrap_or(WatchStatus::Watching);
    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());
    let color = new_status.color();

    match target {
        ResolvedWatchlistMedia::Anime(a) => {
            db.upsert_watchlist_entry(user_id, a.id, new_status.as_db_str(), progress, score).await?;

            let site = a.site_url.as_deref().unwrap_or("https://anilist.co");
            let mut desc = format!("### [**{}**]({})\n\n", a.display_title(), site);
            desc.push_str("**Type:** `Anime`\n");
            desc.push_str(&format!("**Status:** {}\n", new_status.badge()));

            if let Some(prog) = progress {
                let p_bar = anime_ui::make_progress_bar(prog, a.episodes, 8);
                if let Some(total) = a.episodes {
                    if !p_bar.is_empty() {
                        desc.push_str(&format!("**Progress:** {p_bar} **{prog}** / {total} eps\n"));
                    } else {
                        desc.push_str(&format!("**Progress:** **{prog}** / {total} eps\n"));
                    }
                } else {
                    desc.push_str(&format!("**Progress:** Episode **{prog}**\n"));
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
                .footer(serenity::CreateEmbedFooter::new("Updated successfully • Use /watchlist view to see your list"));

            if let Some(ref cover) = a.cover_image {
                if let Some(img_url) = cover.best_url() {
                    embed = embed.thumbnail(img_url);
                }
            }

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
        ResolvedWatchlistMedia::Movie(m) => {
            db.upsert_media_watchlist_entry(user_id, &m.id(), new_status.as_db_str(), progress, score).await?;

            let imdb_link = format!("https://www.imdb.com/title/{}/", m.id());
            let mut desc = format!("### [**{}**]({})\n\n", m.name, imdb_link);
            desc.push_str("**Type:** `Movie`\n");
            desc.push_str(&format!("**Status:** {}\n", new_status.badge()));

            if let Some(prog) = progress {
                desc.push_str(&format!("**Progress:** **{prog}** minutes\n"));
            }

            if let Some(s) = score {
                desc.push_str(&format!("**Rating:** ⭐ **{s}**/10\n"));
            }

            let mut embed = serenity::CreateEmbed::new()
                .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                .title("Watchlist Updated")
                .description(desc)
                .color(color)
                .footer(serenity::CreateEmbedFooter::new("Updated successfully • Use /watchlist view to see your list"));

            if let Some(ref poster) = m.poster {
                embed = embed.thumbnail(poster);
            }

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
        ResolvedWatchlistMedia::Series(s) => {
            db.upsert_media_watchlist_entry(user_id, &s.id(), new_status.as_db_str(), progress, score).await?;

            let imdb_link = format!("https://www.imdb.com/title/{}/", s.id());
            let mut desc = format!("### [**{}**]({})\n\n", s.name, imdb_link);
            desc.push_str("**Type:** `TV Series`\n");
            desc.push_str(&format!("**Status:** {}\n", new_status.badge()));

            if let Some(prog) = progress {
                desc.push_str(&format!("**Progress:** Episode **{prog}**\n"));
            }

            if let Some(s) = score {
                desc.push_str(&format!("**Rating:** ⭐ **{s}**/10\n"));
            }

            let mut embed = serenity::CreateEmbed::new()
                .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                .title("Watchlist Updated")
                .description(desc)
                .color(color)
                .footer(serenity::CreateEmbedFooter::new("Updated successfully • Use /watchlist view to see your list"));

            if let Some(ref poster) = s.poster {
                embed = embed.thumbnail(poster);
            }

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
    }

    Ok(())
}

/// Remove an item from your watchlist.
#[poise::command(slash_command)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Title to remove from your watchlist"]
    #[autocomplete = "autocomplete_user_watchlist"]
    media: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let http = &ctx.data().http;
    let db = &ctx.data().db;
    let user_id = ctx.author().id.get();

    let target = match resolve_input_media(http, &media).await? {
        Some(m) => m,
        None => {
            let entries = db.get_unified_watchlist(user_id, None, None).await.unwrap_or_default();
            let matched = entries.into_iter().find(|e| {
                e.title.eq_ignore_ascii_case(&media)
                    || e.media_id == media
                    || e.title.to_lowercase().contains(&media.to_lowercase())
            });

            if let Some(e) = matched {
                let formatted = format!("{}:{}", e.media_type, e.media_id);
                match resolve_input_media(http, &formatted).await? {
                    Some(m) => m,
                    None => {
                        ctx.send(
                            poise::CreateReply::default()
                                .content(format!("Could not resolve `{}` from your watchlist.", e.title))
                                .ephemeral(true),
                        )
                        .await?;
                        return Ok(());
                    }
                }
            } else {
                ctx.send(
                    poise::CreateReply::default()
                        .content(format!("Could not find `{media}` on your watchlist."))
                        .ephemeral(true),
                )
                .await?;
                return Ok(());
            }
        }
    };

    let user_name = ctx.author().global_name.as_deref().unwrap_or(&ctx.author().name);
    let avatar = anime_ui::user_avatar_url(ctx.author());

    match target {
        ResolvedWatchlistMedia::Anime(a) => {
            let removed = db.remove_from_watchlist(user_id, a.id).await?;
            let site = a.site_url.as_deref().unwrap_or("https://anilist.co");
            if removed {
                let embed = serenity::CreateEmbed::new()
                    .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                    .title("Removed from Watchlist")
                    .description(format!("🗑️ Removed [**{}**]({}) from your watchlist.", a.display_title(), site))
                    .color(anime_ui::COLOR_DROPPED);
                ctx.send(poise::CreateReply::default().embed(embed)).await?;
            } else {
                let embed = serenity::CreateEmbed::new()
                    .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                    .description(format!("⚠️ [**{}**]({}) was not on your watchlist.", a.display_title(), site))
                    .color(anime_ui::COLOR_ON_HOLD);
                ctx.send(poise::CreateReply::default().embed(embed)).await?;
            }
        }
        ResolvedWatchlistMedia::Movie(m) => {
            let removed = db.remove_from_media_watchlist(user_id, &m.id()).await?;
            let imdb_link = format!("https://www.imdb.com/title/{}/", m.id());
            if removed {
                let embed = serenity::CreateEmbed::new()
                    .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                    .title("Removed from Watchlist")
                    .description(format!("🗑️ Removed [**{}**]({}) from your watchlist.", m.name, imdb_link))
                    .color(anime_ui::COLOR_DROPPED);
                ctx.send(poise::CreateReply::default().embed(embed)).await?;
            } else {
                let embed = serenity::CreateEmbed::new()
                    .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                    .description(format!("⚠️ [**{}**]({}) was not on your watchlist.", m.name, imdb_link))
                    .color(anime_ui::COLOR_ON_HOLD);
                ctx.send(poise::CreateReply::default().embed(embed)).await?;
            }
        }
        ResolvedWatchlistMedia::Series(s) => {
            let removed = db.remove_from_media_watchlist(user_id, &s.id()).await?;
            let imdb_link = format!("https://www.imdb.com/title/{}/", s.id());
            if removed {
                let embed = serenity::CreateEmbed::new()
                    .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                    .title("Removed from Watchlist")
                    .description(format!("🗑️ Removed [**{}**]({}) from your watchlist.", s.name, imdb_link))
                    .color(anime_ui::COLOR_DROPPED);
                ctx.send(poise::CreateReply::default().embed(embed)).await?;
            } else {
                let embed = serenity::CreateEmbed::new()
                    .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
                    .description(format!("⚠️ [**{}**]({}) was not on your watchlist.", s.name, imdb_link))
                    .color(anime_ui::COLOR_ON_HOLD);
                ctx.send(poise::CreateReply::default().embed(embed)).await?;
            }
        }
    }

    Ok(())
}

/// Builds the embed and pagination action row for a given page of the watchlist.
pub fn render_watchlist_view(
    caller_id: u64,
    target_user: &serenity::User,
    entries: &[UnifiedWatchlistEntry],
    mut page: usize,
    status_filter: Option<WatchStatus>,
    type_filter: Option<WatchlistTypeFilter>,
) -> (serenity::CreateEmbed, Option<serenity::CreateActionRow>) {
    let user_name = target_user.global_name.as_deref().unwrap_or(&target_user.name);
    let avatar = anime_ui::user_avatar_url(target_user);

    if entries.is_empty() {
        let msg = match (status_filter, type_filter) {
            (Some(s), Some(t)) => format!("No {} listed under **{}**.", t.display_name(), s.display_label()),
            (Some(s), None) => format!("No titles listed under **{}**.", s.display_label()),
            (None, Some(t)) => format!("No {} on this watchlist.", t.display_name()),
            (None, None) => "Watchlist is currently empty.\nUse `/watchlist add <title>` or `/anime`, `/movie`, `/tv` to add shows!".to_string(),
        };

        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
            .description(msg)
            .color(COLOR_UNIFIED_WATCHLIST);

        return (embed, None);
    }

    let total_count = entries.len();
    let total_pages = (total_count + ITEMS_PER_PAGE - 1) / ITEMS_PER_PAGE;
    if page < 1 {
        page = 1;
    }
    if page > total_pages {
        page = total_pages;
    }

    let start = (page - 1) * ITEMS_PER_PAGE;
    let end = std::cmp::min(start + ITEMS_PER_PAGE, total_count);
    let page_entries = &entries[start..end];

    let title = match (status_filter, type_filter) {
        (Some(s), Some(t)) => format!("{} — {} ({})", user_name, s.display_label(), t.display_name()),
        (Some(s), None) => format!("{} — {}", user_name, s.display_label()),
        (None, Some(t)) => format!("{}'s {} Watchlist", user_name, t.display_name()),
        (None, None) => format!("{user_name}'s Unified Watchlist"),
    };

    let embed_color = match status_filter {
        Some(s) => s.color(),
        None => match type_filter {
            Some(WatchlistTypeFilter::Anime) => anime_ui::COLOR_ANILIST,
            Some(WatchlistTypeFilter::Movie) => 0xE50914,
            Some(WatchlistTypeFilter::Series) => 0x00A8E8,
            _ => COLOR_UNIFIED_WATCHLIST,
        },
    };

    let mut description = String::new();

    // Summary banner on page 1 if viewing all
    if page == 1 && status_filter.is_none() && (type_filter.is_none() || type_filter == Some(WatchlistTypeFilter::All)) {
        let mut anime_cnt = 0;
        let mut movie_cnt = 0;
        let mut series_cnt = 0;
        let mut watching_cnt = 0;
        let mut completed_cnt = 0;

        for e in entries {
            match e.media_type.as_str() {
                "anime" => anime_cnt += 1,
                "movie" => movie_cnt += 1,
                "series" => series_cnt += 1,
                _ => {}
            }
            match e.status.as_str() {
                "watching" => watching_cnt += 1,
                "completed" => completed_cnt += 1,
                _ => {}
            }
        }

        let mut pills = Vec::new();
        if anime_cnt > 0 { pills.push(format!("🌸 **{anime_cnt}** Anime")); }
        if movie_cnt > 0 { pills.push(format!("🎬 **{movie_cnt}** Movies")); }
        if series_cnt > 0 { pills.push(format!("📺 **{series_cnt}** TV")); }
        if watching_cnt > 0 { pills.push(format!("▶️ **{watching_cnt}** Watching")); }
        if completed_cnt > 0 { pills.push(format!("✅ **{completed_cnt}** Completed")); }

        if !pills.is_empty() {
            description.push_str(&pills.join("  •  "));
            description.push_str("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n");
        }
    }

    for (idx, entry) in page_entries.iter().enumerate() {
        let absolute_idx = start + idx + 1;
        let status_enum = WatchStatus::from_db_str(&entry.status);
        let year_str = entry.year.as_deref().map(|y| format!(" ({y})")).unwrap_or_default();

        let type_tag = match entry.media_type.as_str() {
            "anime" => "Anime",
            "movie" => "Movie",
            "series" => "TV",
            _ => "Media",
        };

        let title_line = format!(
            "`{:2}` `[{}]` [**{}{}**]({})\n",
            absolute_idx, type_tag, entry.title, year_str, entry.site_url
        );

        let mut details = Vec::new();
        details.push(status_enum.display_label().to_string());

        if entry.status == "completed" {
            if let Some(total) = entry.total_progress {
                details.push(format!("**{total}/{total}** eps"));
            } else if entry.progress > 0 {
                if entry.media_type == "series" {
                    details.push(format!("**{}** eps", entry.progress));
                } else if entry.media_type == "movie" {
                    details.push(format!("**{}**m", entry.progress));
                }
            }
        } else if entry.progress > 0 || entry.status == "watching" {
            if entry.media_type == "anime" {
                let p_bar = anime_ui::make_progress_bar(entry.progress, entry.total_progress, 6);
                if let Some(total) = entry.total_progress {
                    if !p_bar.is_empty() {
                        details.push(format!("{p_bar} **{}**/{}", entry.progress, total));
                    } else {
                        details.push(format!("**{}/{}** eps", entry.progress, total));
                    }
                } else if entry.progress > 0 {
                    details.push(format!("Ep **{}**", entry.progress));
                }
            } else if entry.media_type == "series" {
                if entry.progress > 0 {
                    details.push(format!("Ep **{}**", entry.progress));
                }
            } else if entry.media_type == "movie" && entry.progress > 0 {
                details.push(format!("**{}**m", entry.progress));
            }
        } else if let Some(total) = entry.total_progress {
            details.push(format!("{total} eps"));
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

    let footer_text = if total_pages > 1 {
        format!("Page {page} of {total_pages} • {total_count} total items • Use /watchlist update to log progress")
    } else {
        format!("{total_count} total items • Use /watchlist update to log progress")
    };

    let mut embed = serenity::CreateEmbed::new()
        .author(serenity::CreateEmbedAuthor::new(format!("{user_name}'s Watchlist")).icon_url(avatar))
        .title(title)
        .description(description)
        .color(embed_color)
        .footer(serenity::CreateEmbedFooter::new(footer_text));

    if let Some(poster) = page_entries.first().and_then(|e| e.poster_url.as_deref()) {
        embed = embed.thumbnail(poster);
    }

    let action_row = if total_pages > 1 {
        let status_str = status_filter.map(|s| s.as_db_str()).unwrap_or("all");
        let type_str = type_filter.and_then(|t| t.as_db_str()).unwrap_or("all");
        let target_id = target_user.id.get();

        let prev_page = if page > 1 { page - 1 } else { 1 };
        let next_page = if page < total_pages { page + 1 } else { total_pages };

        let prev_id = format!("wl_page:{caller_id}:{target_id}:{prev_page}:{status_str}:{type_str}");
        let next_id = format!("wl_page:{caller_id}:{target_id}:{next_page}:{status_str}:{type_str}");

        let prev_btn = serenity::CreateButton::new(prev_id)
            .label("◀ Previous")
            .style(serenity::ButtonStyle::Secondary)
            .disabled(page <= 1);

        let next_btn = serenity::CreateButton::new(next_id)
            .label("Next ▶")
            .style(serenity::ButtonStyle::Secondary)
            .disabled(page >= total_pages);

        Some(serenity::CreateActionRow::Buttons(vec![prev_btn, next_btn]))
    } else {
        None
    };

    (embed, action_row)
}
