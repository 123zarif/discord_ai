use poise::serenity_prelude as serenity;
use crate::anilist::{self, AnimeMedia};
use crate::cinemeta::{self, CinemetaMedia};
use crate::discord::commands::anime_ui::{self, COLOR_RECOMMEND};
use crate::discord::{Context, Error};

#[derive(Debug, poise::ChoiceParameter, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedMediaTypeChoice {
    #[name = "Anime"]
    Anime,
    #[name = "Movie"]
    Movie,
    #[name = "TV Series"]
    Series,
}

#[allow(dead_code)]
impl UnifiedMediaTypeChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Anime => "anime",
            Self::Movie => "movie",
            Self::Series => "series",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Anime => "Anime",
            Self::Movie => "Movie",
            Self::Series => "TV Series",
        }
    }
}

/// Autocomplete provider supporting Anime, Movies, and TV series in parallel.
pub async fn autocomplete_recommend_media(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let clean = partial.trim();
    if clean.is_empty() {
        return serenity::CreateAutocompleteResponse::new().set_choices(vec![]);
    }

    let http = &ctx.data().http;

    let (anime_res, movies_res, series_res) = tokio::join!(
        anilist::search_anime(http, clean, 4),
        cinemeta::search_movies(http, clean, 3),
        cinemeta::search_series(http, clean, 3),
    );

    let mut choices = Vec::new();

    if let Ok(anime_list) = anime_res {
        for a in anime_list {
            let mut label = format!("[Anime] {}", a.display_label());
            if label.len() > 100 {
                label.truncate(97);
                label.push_str("...");
            }
            choices.push(serenity::AutocompleteChoice::new(label, format!("anime:{}", a.id)));
        }
    }

    if let Ok(movies) = movies_res {
        for m in movies {
            let mut label = format!("[Movie] {}", m.display_label());
            if label.len() > 100 {
                label.truncate(97);
                label.push_str("...");
            }
            choices.push(serenity::AutocompleteChoice::new(label, format!("movie:{}", m.id)));
        }
    }

    if let Ok(series) = series_res {
        for s in series {
            let mut label = format!("[TV] {}", s.display_label());
            if label.len() > 100 {
                label.truncate(97);
                label.push_str("...");
            }
            choices.push(serenity::AutocompleteChoice::new(label, format!("series:{}", s.id)));
        }
    }

    serenity::CreateAutocompleteResponse::new().set_choices(choices)
}

/// Recommend anime, movies, or TV series to friends and view recommendations.
#[poise::command(
    slash_command,
    subcommands("send", "list", "sent"),
    subcommand_required
)]
pub async fn recommend(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

enum ResolvedMedia {
    Anime(AnimeMedia),
    Movie(CinemetaMedia),
    Series(CinemetaMedia),
}

/// Recommend an anime, movie, or TV series to another user with an optional personal note.
#[poise::command(slash_command)]
pub async fn send(
    ctx: Context<'_>,
    #[description = "User you are recommending to"]
    user: serenity::User,
    #[description = "Title to recommend (search anime, movies, and TV series)"]
    #[autocomplete = "autocomplete_recommend_media"]
    media: String,
    #[description = "Media type (optional override if typing title manually)"]
    media_type: Option<UnifiedMediaTypeChoice>,
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
                .content("You cannot recommend media to yourself! Use `/watchlist add` or `/movielist add` instead.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let http = &ctx.data().http;
    let db = &ctx.data().db;
    let input = media.trim();

    // Check if input came from autocomplete (e.g. "anime:16498", "movie:tt1375666", "series:tt0903747")
    let resolved = if let Some(stripped) = input.strip_prefix("anime:") {
        if let Ok(id) = stripped.parse::<i32>() {
            anilist::get_anime_by_id(http, id).await?.map(ResolvedMedia::Anime)
        } else {
            None
        }
    } else if let Some(imdb_id) = input.strip_prefix("movie:") {
        cinemeta::get_movie_by_id(http, imdb_id).await?.map(ResolvedMedia::Movie)
    } else if let Some(imdb_id) = input.strip_prefix("series:") {
        cinemeta::get_series_by_id(http, imdb_id).await?.map(ResolvedMedia::Series)
    } else if input.starts_with("tt") && input.chars().skip(2).all(|c| c.is_ascii_digit()) {
        // Raw IMDb ID without prefix
        if let Some(m) = cinemeta::get_movie_by_id(http, input).await? {
            Some(ResolvedMedia::Movie(m))
        } else if let Some(s) = cinemeta::get_series_by_id(http, input).await? {
            Some(ResolvedMedia::Series(s))
        } else {
            None
        }
    } else {
        // Plain text search
        match media_type {
            Some(UnifiedMediaTypeChoice::Anime) => {
                anilist::get_anime_by_title(http, input).await?.map(ResolvedMedia::Anime)
            }
            Some(UnifiedMediaTypeChoice::Movie) => {
                let res = cinemeta::search_movies(http, input, 1).await?;
                if let Some(first) = res.into_iter().next() {
                    cinemeta::get_movie_by_id(http, &first.id).await?.map(ResolvedMedia::Movie)
                } else {
                    None
                }
            }
            Some(UnifiedMediaTypeChoice::Series) => {
                let res = cinemeta::search_series(http, input, 1).await?;
                if let Some(first) = res.into_iter().next() {
                    cinemeta::get_series_by_id(http, &first.id).await?.map(ResolvedMedia::Series)
                } else {
                    None
                }
            }
            None => {
                // Try AniList first, then movie, then series
                if let Some(a) = anilist::get_anime_by_title(http, input).await? {
                    Some(ResolvedMedia::Anime(a))
                } else {
                    let m_res = cinemeta::search_movies(http, input, 1).await.unwrap_or_default();
                    if let Some(first) = m_res.into_iter().next() {
                        cinemeta::get_movie_by_id(http, &first.id).await?.map(ResolvedMedia::Movie)
                    } else {
                        let s_res = cinemeta::search_series(http, input, 1).await.unwrap_or_default();
                        if let Some(first) = s_res.into_iter().next() {
                            cinemeta::get_series_by_id(http, &first.id).await?.map(ResolvedMedia::Series)
                        } else {
                            None
                        }
                    }
                }
            }
        }
    };

    let target_media = match resolved {
        Some(m) => m,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("Could not find media matching `{input}`."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    let (m_type, m_id, title, poster_url, embed, _site_url) = match target_media {
        ResolvedMedia::Anime(a) => {
            db.upsert_anime_cache(&a).await?;
            let site = a.site_url.clone().unwrap_or_else(|| "https://anilist.co".to_string());
            let poster = a.cover_image.as_ref().and_then(|c| c.best_url().map(|s| s.to_string()));

            let mut desc = String::new();
            if let Some(ref note_text) = note {
                desc.push_str(&format!("> 💬 *\"{note_text}\"*\n\n"));
            }
            let raw_desc = a.clean_description();
            let (truncated, was_cut) = anime_ui::truncate_synopsis(&raw_desc, 300);
            desc.push_str(&truncated);
            if was_cut {
                desc.push_str(&format!("... [Read more]({site})"));
            }

            let format_str = anime_ui::clean_format(a.format.as_deref());
            let status_str = anime_ui::clean_airing_status(a.status.as_deref());
            let ep_str = a.episodes.map(|ep| format!("**{ep}** eps")).unwrap_or_else(|| "Ongoing".to_string());
            let score_str = a.average_score.map(|s| format!("⭐ **{s}%**")).unwrap_or_else(|| "—".to_string());

            let mut em = serenity::CreateEmbed::new()
                .author(serenity::CreateEmbedAuthor::new(format!("Recommended by {sender_name}")).icon_url(sender_avatar.clone()))
                .title(format!("[Anime] {}", a.display_title()))
                .url(&site)
                .description(desc)
                .color(COLOR_RECOMMEND)
                .field("Rating", score_str, true)
                .field("Format", format!("{format_str} • {ep_str} • {status_str}"), true);

            if !a.genres.is_empty() {
                em = em.field("Genres", a.genres.join(" • "), false);
            }
            if let Some(ref p) = poster {
                em = em.thumbnail(p);
            }
            if let Some(ref banner) = a.banner_image {
                em = em.image(banner);
            }

            em = em.footer(serenity::CreateEmbedFooter::new("Click below to add to your anime watchlist"));
            ("anime", a.id.to_string(), a.display_title().to_string(), poster, em, site)
        }
        ResolvedMedia::Movie(m) => {
            db.upsert_media_cache(&m).await?;
            let site = format!("https://www.imdb.com/title/{}/", m.id());
            let poster = m.poster.clone();

            let mut desc = String::new();
            if let Some(ref note_text) = note {
                desc.push_str(&format!("> 💬 *\"{note_text}\"*\n\n"));
            }
            let raw_desc = m.clean_description();
            let (truncated, was_cut) = anime_ui::truncate_synopsis(&raw_desc, 300);
            desc.push_str(&truncated);
            if was_cut {
                desc.push_str(&format!("... [IMDb]({site})"));
            }

            let rating_str = m.imdb_rating.as_ref().map(|r| format!("⭐ **{r}/10**")).unwrap_or_else(|| "—".to_string());
            let runtime_str = m.runtime.as_ref().map(|r| r.as_str()).unwrap_or("—");

            let mut em = serenity::CreateEmbed::new()
                .author(serenity::CreateEmbedAuthor::new(format!("Recommended by {sender_name}")).icon_url(sender_avatar.clone()))
                .title(format!("[Movie] {}", m.name))
                .url(&site)
                .description(desc)
                .color(0xE50914) // Movie red
                .field("IMDb Rating", rating_str, true)
                .field("Runtime", runtime_str.to_string(), true);

            if !m.genres.is_empty() {
                em = em.field("Genres", m.genres.join(" • "), false);
            }
            if let Some(ref p) = poster {
                em = em.thumbnail(p);
            }
            if let Some(ref banner) = m.background {
                em = em.image(banner);
            }

            em = em.footer(serenity::CreateEmbedFooter::new("Click below to add to your movie watchlist"));
            ("movie", m.id().to_string(), m.name.clone(), poster, em, site)
        }
        ResolvedMedia::Series(s) => {
            db.upsert_media_cache(&s).await?;
            let site = format!("https://www.imdb.com/title/{}/", s.id());
            let poster = s.poster.clone();

            let mut desc = String::new();
            if let Some(ref note_text) = note {
                desc.push_str(&format!("> 💬 *\"{note_text}\"*\n\n"));
            }
            let raw_desc = s.clean_description();
            let (truncated, was_cut) = anime_ui::truncate_synopsis(&raw_desc, 300);
            desc.push_str(&truncated);
            if was_cut {
                desc.push_str(&format!("... [IMDb]({site})"));
            }

            let rating_str = s.imdb_rating.as_ref().map(|r| format!("⭐ **{r}/10**")).unwrap_or_else(|| "—".to_string());
            let status_str = s.status.as_ref().or(s.runtime.as_ref()).map(|st| st.as_str()).unwrap_or("TV Series");

            let mut em = serenity::CreateEmbed::new()
                .author(serenity::CreateEmbedAuthor::new(format!("Recommended by {sender_name}")).icon_url(sender_avatar.clone()))
                .title(format!("[TV Series] {}", s.name))
                .url(&site)
                .description(desc)
                .color(0x00A8E8) // TV Cyan
                .field("IMDb Rating", rating_str, true)
                .field("Status", status_str.to_string(), true);

            if !s.genres.is_empty() {
                em = em.field("Genres", s.genres.join(" • "), false);
            }
            if let Some(ref p) = poster {
                em = em.thumbnail(p);
            }
            if let Some(ref banner) = s.background {
                em = em.image(banner);
            }

            em = em.footer(serenity::CreateEmbedFooter::new("Click below to add to your TV watchlist"));
            ("series", s.id().to_string(), s.name.clone(), poster, em, site)
        }
    };

    // Store unified recommendation in database
    let rec_id = db
        .create_unified_recommendation(
            sender.id.get(),
            user.id.get(),
            m_type,
            &m_id,
            &title,
            poster_url.as_deref(),
            note.as_deref(),
        )
        .await?;

    // Also insert into legacy anime recommendations if anime for backwards compatibility
    if m_type == "anime" {
        if let Ok(anime_id) = m_id.parse::<i32>() {
            let _ = db.create_recommendation(sender.id.get(), user.id.get(), anime_id, note.as_deref()).await;
        }
    }

    // Interactive Button for recipient
    let button_id = format!("rec_add:{}:{}:{}:{}", rec_id, user.id.get(), m_type, m_id);
    let add_button = serenity::CreateButton::new(button_id)
        .label("Add to Watchlist")
        .style(serenity::ButtonStyle::Primary);

    let action_row = serenity::CreateActionRow::Buttons(vec![add_button]);
    let type_label = match m_type {
        "anime" => "an anime",
        "movie" => "a movie",
        "series" => "a TV series",
        _ => "a title",
    };
    let message_text = format!("<@{}>, **{}** recommended {type_label} for you!", user.id, sender_name);

    ctx.send(
        poise::CreateReply::default()
            .content(message_text)
            .embed(embed)
            .components(vec![action_row]),
    )
    .await?;

    Ok(())
}

/// View recommendations you have received from other users.
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

    let recs = db.get_received_unified_recommendations(recipient_id, sender_filter).await?;

    let caller = ctx.author();
    let caller_name = caller.global_name.as_deref().unwrap_or(&caller.name);
    let caller_avatar = anime_ui::user_avatar_url(caller);

    if recs.is_empty() {
        let msg = match from {
            Some(u) => format!("You haven't received any recommendations from **{}**.", u.name),
            None => "You haven't received any recommendations yet!\nFriends can send you one with `/recommend send`.".to_string(),
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
            "`Added to Watchlist`"
        } else {
            "`Pending`"
        };

        let (type_tag, site_url) = match rec.media_type.as_str() {
            "anime" => ("Anime", format!("https://anilist.co/anime/{}", rec.media_id)),
            "movie" | "series" => (if rec.media_type == "series" { "TV" } else { "Movie" }, format!("https://www.imdb.com/title/{}/", rec.media_id)),
            _ => ("Media", "#".to_string()),
        };

        let title_line = format!("`{:2}` `[{type_tag}]` [**{}**]({})\n", idx + 1, rec.title, site_url);
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

    if let Some(poster) = recs.first().and_then(|r| r.poster_url.as_deref()) {
        embed = embed.thumbnail(poster);
    }

    embed = embed.footer(serenity::CreateEmbedFooter::new(format!(
        "Total: {total} • Click [Add to Watchlist] on the recommendation card to accept"
    )));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// View recommendations you have sent to other users.
#[poise::command(slash_command)]
pub async fn sent(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let db = &ctx.data().db;
    let sender_id = ctx.author().id.get();

    let recs = db.get_sent_unified_recommendations(sender_id).await?;

    let caller = ctx.author();
    let caller_name = caller.global_name.as_deref().unwrap_or(&caller.name);
    let caller_avatar = anime_ui::user_avatar_url(caller);

    if recs.is_empty() {
        let embed = serenity::CreateEmbed::new()
            .author(serenity::CreateEmbedAuthor::new(format!("{caller_name}'s Sent Recommendations")).icon_url(caller_avatar))
            .description("You haven't sent any recommendations yet!\nUse `/recommend send` to recommend anime, movies, or TV series to a friend.")
            .color(0xF39C12);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    let mut description = String::new();
    let total = recs.len();

    for (idx, rec) in recs.iter().take(12).enumerate() {
        let status_badge = if rec.status == "added" {
            "`Added`"
        } else {
            "`Pending`"
        };

        let (type_tag, site_url) = match rec.media_type.as_str() {
            "anime" => ("Anime", format!("https://anilist.co/anime/{}", rec.media_id)),
            "movie" | "series" => (if rec.media_type == "series" { "TV" } else { "Movie" }, format!("https://www.imdb.com/title/{}/", rec.media_id)),
            _ => ("Media", "#".to_string()),
        };

        let title_line = format!("`{:2}` `[{type_tag}]` [**{}**]({})\n", idx + 1, rec.title, site_url);
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
        .title("Sent Recommendations")
        .description(description)
        .color(0xF39C12);

    if let Some(poster) = recs.first().and_then(|r| r.poster_url.as_deref()) {
        embed = embed.thumbnail(poster);
    }

    embed = embed.footer(serenity::CreateEmbedFooter::new(format!(
        "Total Sent: {total}"
    )));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
