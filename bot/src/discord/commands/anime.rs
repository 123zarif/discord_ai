use poise::serenity_prelude as serenity;
use crate::anilist::{self, AnimeMedia};
use crate::discord::{Context, Error};

/// Autocomplete provider for anime titles querying the AniList GraphQL API in real time.
pub async fn autocomplete_anime(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let clean = partial.trim();
    if clean.is_empty() {
        return serenity::CreateAutocompleteResponse::new().set_choices(vec![]);
    }

    let http = &ctx.data().http;
    match anilist::search_anime(http, clean, 10).await {
        Ok(results) => {
            let choices: Vec<serenity::AutocompleteChoice> = results
                .into_iter()
                .map(|item| {
                    let label = item.display_label();
                    // Pass the AniList numeric ID as string for exact lookup
                    serenity::AutocompleteChoice::new(label, item.id.to_string())
                })
                .collect();
            serenity::CreateAutocompleteResponse::new().set_choices(choices)
        }
        Err(_) => serenity::CreateAutocompleteResponse::new().set_choices(vec![]),
    }
}

/// Search for an anime on AniList and view its synopsis, rating, episodes, and cover art.
#[poise::command(slash_command)]
pub async fn anime(
    ctx: Context<'_>,
    #[description = "Anime title to search on AniList"]
    #[autocomplete = "autocomplete_anime"]
    title: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let http = &ctx.data().http;
    let db = &ctx.data().db;

    // If title was chosen from autocomplete, it will be a numeric AniList ID.
    // Otherwise, search by title string.
    let media = if let Ok(id) = title.trim().parse::<i32>() {
        anilist::get_anime_by_id(http, id).await.map_err(|e| e.to_string())?
    } else {
        anilist::get_anime_by_title(http, title.trim()).await.map_err(|e| e.to_string())?
    };

    let anime = match media {
        Some(a) => a,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("❌ Could not find anime `{title}` on AniList."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Cache the anime in PostgreSQL
    let _ = db.upsert_anime_cache(&anime).await;

    let embed = build_anime_embed(&anime);

    // Interactive button: Anyone can click to add this anime to their own Plan to Watch list
    let button_id = format!("anime:add:{}", anime.id);
    let add_button = serenity::CreateButton::new(button_id)
        .label("Add to Watchlist")
        .style(serenity::ButtonStyle::Primary);

    let action_row = serenity::CreateActionRow::Buttons(vec![add_button]);

    ctx.send(
        poise::CreateReply::default()
            .embed(embed)
            .components(vec![action_row]),
    )
    .await?;

    Ok(())
}

/// Builds a modern, clean Discord embed for an anime.
pub fn build_anime_embed(anime: &AnimeMedia) -> serenity::CreateEmbed {
    use crate::discord::commands::anime_ui::{self, COLOR_ANILIST};

    let site_url = anime.site_url.as_deref().unwrap_or("https://anilist.co");

    // Format author header: e.g. "AniList • TV Series (2024)"
    let format_str = anime_ui::clean_format(anime.format.as_deref());
    let mut author_line = format!("AniList • {format_str}");
    if let Some(yr) = anime.season_year {
        author_line.push_str(&format!(" ({yr})"));
    }

    // Prepare description: Romaji subtitle (if English title is used) + truncated synopsis
    let mut desc = String::new();
    if let Some(ref eng) = anime.title.english {
        if !eng.trim().is_empty() && eng.trim() != anime.title.romaji.trim() {
            desc.push_str(&format!("*Romaji: {}*\n\n", anime.title.romaji));
        }
    }

    let raw_desc = anime.clean_description();
    let (truncated, was_cut) = anime_ui::truncate_synopsis(&raw_desc, 350);
    desc.push_str(&truncated);
    if was_cut {
        desc.push_str(&format!("... [Read more]({site_url})"));
    }

    let mut embed = serenity::CreateEmbed::new()
        .author(
            serenity::CreateEmbedAuthor::new(author_line)
                .icon_url("https://anilist.co/img/icons/android-chrome-512x512.png")
                .url(site_url),
        )
        .title(anime.display_title())
        .url(site_url)
        .description(desc)
        .color(COLOR_ANILIST);

    // Cover thumbnail and banner image
    if let Some(ref cover) = anime.cover_image {
        if let Some(img_url) = cover.best_url() {
            embed = embed.thumbnail(img_url);
        }
    }

    if let Some(ref banner) = anime.banner_image {
        embed = embed.image(banner);
    }

    // Field 1: Score & Status
    let score_str = anime
        .average_score
        .map(|s| format!("⭐ **{s}%**"))
        .unwrap_or_else(|| "—".to_string());
    let status_str = anime_ui::clean_airing_status(anime.status.as_deref());

    // Field 2: Episodes
    let ep_str = anime
        .episodes
        .map(|ep| format!("**{ep}** eps"))
        .unwrap_or_else(|| "Ongoing".to_string());

    embed = embed.field("Rating", score_str, true);
    embed = embed.field("Format & Episodes", format!("{ep_str} • {status_str}"), true);

    // Field 3: Genres
    if !anime.genres.is_empty() {
        embed = embed.field("Genres", anime.genres.join(" • "), false);
    }

    embed = embed.footer(serenity::CreateEmbedFooter::new(format!(
        "AniList ID: {} • Click below to add to your list",
        anime.id
    )));

    embed
}
