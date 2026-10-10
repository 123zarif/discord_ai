use poise::serenity_prelude as serenity;
use crate::cinemeta::{self, CinemetaMedia};
use crate::discord::{Context, Error};

const COLOR_TV: u32 = 0x00A8E8; // Cyan / Cool Blue

/// Autocomplete provider for TV series querying Cinemeta in real time.
pub async fn autocomplete_tv(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let clean = partial.trim();
    if clean.is_empty() {
        return serenity::CreateAutocompleteResponse::new().set_choices(vec![]);
    }

    let http = &ctx.data().http;
    match cinemeta::search_series(http, clean, 10).await {
        Ok(results) => {
            let choices: Vec<serenity::AutocompleteChoice> = results
                .into_iter()
                .map(|item| {
                    let label = item.display_label();
                    serenity::AutocompleteChoice::new(label, item.id)
                })
                .collect();
            serenity::CreateAutocompleteResponse::new().set_choices(choices)
        }
        Err(_) => serenity::CreateAutocompleteResponse::new().set_choices(vec![]),
    }
}

/// Search for a TV series and view its synopsis, rating, seasons, and poster.
#[poise::command(slash_command)]
pub async fn tv(
    ctx: Context<'_>,
    #[description = "TV series title to search"]
    #[autocomplete = "autocomplete_tv"]
    title: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let http = &ctx.data().http;
    let db = &ctx.data().db;
    let query = title.trim();

    // If title was chosen from autocomplete, it will be an IMDb ID (e.g. "tt0903747")
    let media = if query.starts_with("tt") && query.chars().skip(2).all(|c| c.is_ascii_digit()) {
        cinemeta::get_series_by_id(http, query).await.map_err(|e| e.to_string())?
    } else {
        let results = cinemeta::search_series(http, query, 1).await.map_err(|e| e.to_string())?;
        if let Some(first) = results.into_iter().next() {
            cinemeta::get_series_by_id(http, &first.id).await.map_err(|e| e.to_string())?
        } else {
            None
        }
    };

    let show = match media {
        Some(s) => s,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("Could not find TV series `{title}`."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Cache the series in PostgreSQL
    let _ = db.upsert_media_cache(&show).await;

    let embed = build_tv_embed(&show);

    // Interactive button: Anyone can click to add this TV show to their own Plan to Watch list
    let button_id = format!("tv:add:{}", show.id());
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

/// Builds a clean Discord embed for a TV series.
pub fn build_tv_embed(show: &CinemetaMedia) -> serenity::CreateEmbed {
    let mut author_line = "TV Series".to_string();
    if let Some(yr) = show.display_year() {
        author_line.push_str(&format!(" ({yr})"));
    }

    let mut fields = Vec::new();

    // Rating field
    if let Some(ref rating) = show.imdb_rating {
        fields.push(("IMDb Rating", format!("{rating} / 10"), true));
    }

    // Status / Runtime
    if let Some(ref status) = show.status {
        fields.push(("Status", status.clone(), true));
    } else if let Some(ref runtime) = show.runtime {
        fields.push(("Episode Runtime", runtime.clone(), true));
    }

    // Genres field
    if !show.genres.is_empty() {
        fields.push(("Genres", show.genres.join(", "), true));
    }

    let imdb_link = format!("https://www.imdb.com/title/{}/", show.id());

    let mut embed = serenity::CreateEmbed::new()
        .title(&show.name)
        .url(imdb_link)
        .description(show.clean_description())
        .author(serenity::CreateEmbedAuthor::new(author_line))
        .color(COLOR_TV);

    for (name, val, inline) in fields {
        embed = embed.field(name, val, inline);
    }

    if let Some(ref poster) = show.poster {
        embed = embed.thumbnail(poster);
    }

    if let Some(ref backdrop) = show.background {
        embed = embed.image(backdrop);
    }

    embed
}

