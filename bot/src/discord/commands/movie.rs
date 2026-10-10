use poise::serenity_prelude as serenity;
use crate::cinemeta::{self, CinemetaMedia};
use crate::discord::{Context, Error};

const COLOR_MOVIE: u32 = 0xE50914; // Vibrant Cinema Red

/// Autocomplete provider for movie titles querying Cinemeta in real time.
pub async fn autocomplete_movie(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let clean = partial.trim();
    if clean.is_empty() {
        return serenity::CreateAutocompleteResponse::new().set_choices(vec![]);
    }

    let http = &ctx.data().http;
    match cinemeta::search_movies(http, clean, 10).await {
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

/// Search for a movie and view its synopsis, rating, runtime, and poster.
#[poise::command(slash_command)]
pub async fn movie(
    ctx: Context<'_>,
    #[description = "Movie title to search"]
    #[autocomplete = "autocomplete_movie"]
    title: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let http = &ctx.data().http;
    let db = &ctx.data().db;
    let query = title.trim();

    // If title was picked from autocomplete, it will be an IMDb ID (e.g. "tt1375666")
    let media = if query.starts_with("tt") && query.chars().skip(2).all(|c| c.is_ascii_digit()) {
        cinemeta::get_movie_by_id(http, query).await.map_err(|e| e.to_string())?
    } else {
        let results = cinemeta::search_movies(http, query, 1).await.map_err(|e| e.to_string())?;
        if let Some(first) = results.into_iter().next() {
            cinemeta::get_movie_by_id(http, &first.id).await.map_err(|e| e.to_string())?
        } else {
            None
        }
    };

    let movie = match media {
        Some(m) => m,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("Could not find movie `{title}`."))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // Cache the movie in PostgreSQL
    let _ = db.upsert_media_cache(&movie).await;

    let embed = build_movie_embed(&movie);

    // Interactive button: Anyone can click to add this movie to their own Plan to Watch list
    let button_id = format!("movie:add:{}", movie.id());
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

/// Builds a clean Discord embed for a movie.
pub fn build_movie_embed(movie: &CinemetaMedia) -> serenity::CreateEmbed {
    let mut author_line = "Movie".to_string();
    if let Some(yr) = movie.display_year() {
        author_line.push_str(&format!(" ({yr})"));
    }

    let mut fields = Vec::new();

    // Rating field
    if let Some(ref rating) = movie.imdb_rating {
        fields.push(("IMDb Rating", format!("{rating} / 10"), true));
    }

    // Runtime field
    if let Some(ref runtime) = movie.runtime {
        fields.push(("Runtime", runtime.clone(), true));
    }

    // Genres field
    if !movie.genres.is_empty() {
        fields.push(("Genres", movie.genres.join(", "), true));
    }

    let imdb_link = format!("https://www.imdb.com/title/{}/", movie.id());

    let mut embed = serenity::CreateEmbed::new()
        .title(&movie.name)
        .url(imdb_link)
        .description(movie.clean_description())
        .author(serenity::CreateEmbedAuthor::new(author_line))
        .color(COLOR_MOVIE);

    for (name, val, inline) in fields {
        embed = embed.field(name, val, inline);
    }

    if let Some(ref poster) = movie.poster {
        embed = embed.thumbnail(poster);
    }

    if let Some(ref backdrop) = movie.background {
        embed = embed.image(backdrop);
    }

    embed
}

