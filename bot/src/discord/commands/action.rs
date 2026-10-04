use poise::serenity_prelude as serenity;
use crate::discord::actions::{self, ActionType};
use crate::discord::{Context, Error};

/// Autocomplete provider returning matching actions (up to 25 items to respect Discord limit).
async fn autocomplete_action(
    _ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let partial_lower = partial.trim().to_lowercase();
    let choices: Vec<serenity::AutocompleteChoice> = ActionType::ALL
        .iter()
        .filter(move |act| act.endpoint_name().starts_with(&partial_lower))
        .take(25)
        .map(|act| serenity::AutocompleteChoice::new(act.endpoint_name(), act.endpoint_name()))
        .collect();

    serenity::CreateAutocompleteResponse::new().set_choices(choices)
}

/// Perform an anime action (kiss, hug, pat, etc.) optionally towards another user!
#[poise::command(slash_command)]
pub async fn action(
    ctx: Context<'_>,
    #[description = "The action to perform (e.g. kiss, hug, pat, slap, cuddle)"]
    #[autocomplete = "autocomplete_action"]
    action: String,
    #[description = "Optional user to target with the action"]
    target: Option<serenity::User>,
) -> Result<(), Error> {
    let action_type = match ActionType::from_str_case_insensitive(&action) {
        Some(act) => act,
        None => {
            let available = ActionType::ALL
                .iter()
                .map(|a| format!("`{}`", a.endpoint_name()))
                .collect::<Vec<_>>()
                .join(", ");
            ctx.send(
                poise::CreateReply::default()
                    .content(format!(
                        "❌ Unknown action `{action}`.\nAvailable actions: {available}"
                    ))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    ctx.defer().await?;

    let author = ctx.author();
    let author_name = author.global_name.as_deref().unwrap_or(&author.name);
    let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or(0);

    let media = actions::fetch_action_media(action_type).await;

    let (description, count_text) = if let Some(target_user) = &target {
        let target_name = target_user.global_name.as_deref().unwrap_or(&target_user.name);
        let is_self = target_user.id == author.id;

        let desc = if is_self {
            action_type.format_self(author_name)
        } else {
            action_type.format_targeted(author_name, target_name)
        };

        // Increment interaction count in database
        let count = ctx
            .data()
            .db
            .increment_action_count(
                guild_id,
                author.id.get(),
                target_user.id.get(),
                action_type.endpoint_name(),
            )
            .await
            .unwrap_or(1);

        let times_str = if count == 1 { "time" } else { "times" };
        let count_str = if is_self {
            format!("*(They've done this to themselves {} {})*", count, times_str)
        } else {
            format!(
                "*({} has {} {} {} {})*",
                author_name,
                action_type.past_tense(),
                target_name,
                count,
                times_str
            )
        };

        (desc, Some(count_str))
    } else {
        (action_type.format_solo(author_name), None)
    };

    let mut full_description = description;
    if let Some(c) = count_text {
        full_description.push_str("\n\n");
        full_description.push_str(&c);
    }

    let mut embed = serenity::CreateEmbed::new()
        .description(full_description)
        .image(media.url)
        .color(action_type.embed_color());

    if let Some(anime) = media.anime_name {
        embed = embed.footer(serenity::CreateEmbedFooter::new(format!("Anime: {anime}")));
    }

    let mut reply = poise::CreateReply::default().embed(embed);
    if let Some(target_user) = &target {
        if target_user.id != author.id {
            let row = actions::build_action_buttons(
                author.id.get(),
                target_user.id.get(),
                action_type,
            );
            reply = reply.components(vec![row]);
        }
    }

    ctx.send(reply).await?;
    Ok(())
}
