use poise::serenity_prelude as serenity;
use tracing::{info, trace};
use crate::discord::{AppData, Error};

/// Central Discord event dispatcher.
///
/// Designed with extensible event branches to allow future message collection,
/// chunking, and ingestion pipelines to be added without restructuring client logic.
pub async fn handle_event(
    framework: poise::FrameworkContext<'_, AppData, Error>,
    event: &serenity::FullEvent,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Ready { data_about_bot } => {
            info!("Discord connected");
            info!(
                "Bot ready. Logged in as {} (ID: {})",
                data_about_bot.user.name, data_about_bot.user.id
            );
        }
        serenity::FullEvent::Message { new_message } => {
            // Ignore bot accounts to prevent message feedback loops
            if new_message.author.bot {
                return Ok(());
            }

            // Hook for future message ingestion pipeline:
            // At this foundation stage, we avoid storing messages prematurely.
            trace!(
                "Message received from user {} in channel {}: length {}",
                new_message.author.id,
                new_message.channel_id,
                new_message.content.len()
            );
        }
        serenity::FullEvent::MessageUpdate { event, .. } => {
            trace!("Message updated in channel {}", event.channel_id);
        }
        serenity::FullEvent::MessageDelete { channel_id, deleted_message_id, .. } => {
            trace!("Message {} deleted in channel {}", deleted_message_id, channel_id);
        }
        serenity::FullEvent::GuildCreate { guild, is_new } => {
            if is_new.unwrap_or(false) {
                // Ensure no lingering guild-scoped commands create duplicates in newly joined servers
                let empty_cmds: &[poise::Command<AppData, Error>] = &[];
                let _ = poise::builtins::register_in_guild(
                    framework.serenity_context,
                    empty_cmds,
                    guild.id,
                )
                .await;
            }
        }
        serenity::FullEvent::InteractionCreate { interaction } => {
            if let serenity::Interaction::Component(component) = interaction {
                if let Err(e) = handle_component_interaction(framework, component).await {
                    tracing::error!("Error handling component interaction: {e:?}");
                }
            }
        }
        serenity::FullEvent::Resume { .. } => {
            info!("Discord gateway connection resumed");
        }
        _ => {}
    }

    Ok(())
}

async fn handle_component_interaction(
    framework: poise::FrameworkContext<'_, AppData, Error>,
    component: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    let custom_id = &component.data.custom_id;
    if custom_id.starts_with("action:") {
        handle_action_interaction(framework, component).await
    } else if custom_id.starts_with("anime:") {
        handle_anime_interaction(framework, component).await
    } else if custom_id.starts_with("movie:") {
        handle_movie_interaction(framework, component).await
    } else if custom_id.starts_with("tv:") {
        handle_tv_interaction(framework, component).await
    } else if custom_id.starts_with("rec_add:") {
        handle_unified_rec_interaction(framework, component).await
    } else if custom_id.starts_with("wl_page:") {
        handle_watchlist_page_interaction(framework, component).await
    } else {
        Ok(())
    }
}

async fn handle_action_interaction(
    framework: poise::FrameworkContext<'_, AppData, Error>,
    component: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    use crate::discord::actions::{self, ActionType};

    let custom_id = &component.data.custom_id;

    // Format: action:<back|reject>:<author_id>:<target_id>:<action_name>
    let parts: Vec<&str> = custom_id.split(':').collect();
    if parts.len() != 5 {
        return Ok(());
    }

    let kind = parts[1];
    let orig_author_id: u64 = match parts[2].parse() {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };
    let target_id: u64 = match parts[3].parse() {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };
    let action_name = parts[4];
    let action_type = match ActionType::from_str_case_insensitive(action_name) {
        Some(act) => act,
        None => return Ok(()),
    };

    let ctx = framework.serenity_context;
    let data = framework.user_data().await;
    let clicking_user_id = component.user.id.get();

    // Enforce authorization: only targeted user may respond
    if clicking_user_id != target_id {
        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::Message(
                    serenity::CreateInteractionResponseMessage::new()
                        .content("Only the targeted user can respond to this action!")
                        .ephemeral(true),
                ),
            )
            .await?;
        return Ok(());
    }

    // Acknowledge interaction and remove buttons from original message
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new().components(vec![]),
            ),
        )
        .await?;

    let guild_id = component.guild_id.map(|g| g.get()).unwrap_or(0);
    let target_name = component.user.global_name.as_deref().unwrap_or(&component.user.name);

    match kind {
        "reject" => {
            // Decrement the original interaction counter in PostgreSQL
            let _ = data
                .db
                .decrement_action_count(
                    guild_id,
                    orig_author_id,
                    target_id,
                    action_type.endpoint_name(),
                )
                .await;

            // Fetch rejection media (Nope anime gif)
            let media = actions::fetch_action_media(ActionType::Nope).await;

            let description = format!(
                "**{target_name}** rejected <@{orig_author_id}>'s {}! 🙅",
                action_type.endpoint_name()
            );

            let mut embed = serenity::CreateEmbed::new()
                .description(description)
                .image(media.url)
                .color(0xDC143C); // Crimson Red for rejection

            if let Some(anime) = media.anime_name {
                embed = embed.footer(serenity::CreateEmbedFooter::new(format!("Anime: {anime}")));
            }

            component
                .channel_id
                .send_message(ctx, serenity::CreateMessage::new().embed(embed))
                .await?;
        }
        "back" => {
            let is_done_together = action_type.is_done_together();

            // Mutual actions done together (handhold, highfive, cuddle, dance) do NOT increment on response;
            // Directional actions (kiss, hug, slap, etc.) DO increment count!
            let count = if is_done_together {
                data.db
                    .get_action_count(
                        guild_id,
                        target_id,
                        orig_author_id,
                        action_type.endpoint_name(),
                    )
                    .await
                    .unwrap_or(1)
            } else {
                data.db
                    .increment_action_count(
                        guild_id,
                        target_id,
                        orig_author_id,
                        action_type.endpoint_name(),
                    )
                    .await
                    .unwrap_or(1)
            };

            let media = actions::fetch_action_media(action_type).await;

            let description = format!(
                "**{target_name}** {} <@{orig_author_id}> back! 💕",
                action_type.past_tense()
            );

            let times_str = if count == 1 { "time" } else { "times" };
            let count_text = if is_done_together {
                format!(
                    "*({target_name} and <@{orig_author_id}> have {} {count} {times_str}!)*",
                    action_type.past_tense()
                )
            } else {
                format!(
                    "*({target_name} has {} <@{orig_author_id}> {count} {times_str}!)*",
                    action_type.past_tense()
                )
            };

            let full_description = format!("{description}\n\n{count_text}");

            let mut embed = serenity::CreateEmbed::new()
                .description(full_description)
                .image(media.url)
                .color(action_type.embed_color());

            if let Some(anime) = media.anime_name {
                embed = embed.footer(serenity::CreateEmbedFooter::new(format!("Anime: {anime}")));
            }

            component
                .channel_id
                .send_message(ctx, serenity::CreateMessage::new().embed(embed))
                .await?;
        }
        _ => {}
    }

    Ok(())
}

async fn handle_anime_interaction(
    framework: poise::FrameworkContext<'_, AppData, Error>,
    component: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    let custom_id = &component.data.custom_id;
    let parts: Vec<&str> = custom_id.split(':').collect();
    let ctx = framework.serenity_context;
    let data = framework.user_data().await;
    let clicking_user = &component.user;

    if parts.len() == 3 && parts[1] == "add" {
        // anime:add:<anime_id>
        if let Ok(anime_id) = parts[2].parse::<i32>() {
            data.db
                .upsert_watchlist_entry(clicking_user.id.get(), anime_id, "planning", None, None)
                .await?;

            component
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::Message(
                        serenity::CreateInteractionResponseMessage::new()
                            .content("✅ Added to your **Plan to Watch** list!")
                            .ephemeral(true),
                    ),
                )
                .await?;
        }
    } else if parts.len() == 5 && parts[1] == "rec_add" {
        // anime:rec_add:<rec_id>:<target_user_id>:<anime_id>
        let rec_id: i64 = parts[2].parse().unwrap_or(0);
        let target_user_id: u64 = parts[3].parse().unwrap_or(0);
        let anime_id: i32 = parts[4].parse().unwrap_or(0);

        if clicking_user.id.get() != target_user_id {
            component
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::Message(
                        serenity::CreateInteractionResponseMessage::new()
                            .content("Only the recipient of this recommendation can accept it!")
                            .ephemeral(true),
                    ),
                )
                .await?;
            return Ok(());
        }

        data.db
            .upsert_watchlist_entry(target_user_id, anime_id, "planning", None, None)
            .await?;

        let _ = data.db.mark_recommendation_status(rec_id, "added").await;

        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::Message(
                    serenity::CreateInteractionResponseMessage::new()
                        .content("✅ Accepted recommendation and added to your **Plan to Watch** list!")
                        .ephemeral(true),
                ),
            )
            .await?;
    }

    Ok(())
}

async fn handle_movie_interaction(
    framework: poise::FrameworkContext<'_, AppData, Error>,
    component: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    let custom_id = &component.data.custom_id;
    let parts: Vec<&str> = custom_id.split(':').collect();
    let ctx = framework.serenity_context;
    let data = framework.user_data().await;
    let clicking_user = &component.user;

    if parts.len() == 3 && parts[1] == "add" {
        let imdb_id = parts[2];
        if let Ok(Some(movie)) = crate::cinemeta::get_movie_by_id(&data.http, imdb_id).await {
            let _ = data.db.upsert_media_cache(&movie).await;
        }

        data.db
            .upsert_media_watchlist_entry(clicking_user.id.get(), imdb_id, "planning", None, None)
            .await?;

        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::Message(
                    serenity::CreateInteractionResponseMessage::new()
                        .content("Added movie to your **Plan to Watch** list!")
                        .ephemeral(true),
                ),
            )
            .await?;
    }

    Ok(())
}

async fn handle_tv_interaction(
    framework: poise::FrameworkContext<'_, AppData, Error>,
    component: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    let custom_id = &component.data.custom_id;
    let parts: Vec<&str> = custom_id.split(':').collect();
    let ctx = framework.serenity_context;
    let data = framework.user_data().await;
    let clicking_user = &component.user;

    if parts.len() == 3 && parts[1] == "add" {
        let imdb_id = parts[2];
        if let Ok(Some(show)) = crate::cinemeta::get_series_by_id(&data.http, imdb_id).await {
            let _ = data.db.upsert_media_cache(&show).await;
        }

        data.db
            .upsert_media_watchlist_entry(clicking_user.id.get(), imdb_id, "planning", None, None)
            .await?;

        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::Message(
                    serenity::CreateInteractionResponseMessage::new()
                        .content("Added TV series to your **Plan to Watch** list!")
                        .ephemeral(true),
                ),
            )
            .await?;
    }

    Ok(())
}

async fn handle_unified_rec_interaction(
    framework: poise::FrameworkContext<'_, AppData, Error>,
    component: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    let custom_id = &component.data.custom_id;
    let parts: Vec<&str> = custom_id.split(':').collect();
    let ctx = framework.serenity_context;
    let data = framework.user_data().await;
    let clicking_user = &component.user;

    if parts.len() == 5 && parts[0] == "rec_add" {
        let rec_id: i64 = parts[1].parse().unwrap_or(0);
        let target_user_id: u64 = parts[2].parse().unwrap_or(0);
        let media_type = parts[3];
        let media_id = parts[4];

        if clicking_user.id.get() != target_user_id {
            component
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::Message(
                        serenity::CreateInteractionResponseMessage::new()
                            .content("Only the recipient of this recommendation can accept it!")
                            .ephemeral(true),
                    ),
                )
                .await?;
            return Ok(());
        }

        match media_type {
            "anime" => {
                if let Ok(anime_id) = media_id.parse::<i32>() {
                    data.db
                        .upsert_watchlist_entry(target_user_id, anime_id, "planning", None, None)
                        .await?;
                }
            }
            "movie" | "series" => {
                data.db
                    .upsert_media_watchlist_entry(target_user_id, media_id, "planning", None, None)
                    .await?;
            }
            _ => {}
        }

        let _ = data.db.mark_unified_recommendation_status(rec_id, "added").await;
        let _ = data.db.mark_recommendation_status(rec_id, "added").await;

        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::Message(
                    serenity::CreateInteractionResponseMessage::new()
                        .content("Accepted recommendation and added to your **Plan to Watch** list!")
                        .ephemeral(true),
                ),
            )
            .await?;
    }

    Ok(())
}

async fn handle_watchlist_page_interaction(
    framework: poise::FrameworkContext<'_, AppData, Error>,
    component: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    use crate::discord::commands::watchlist::{render_watchlist_view, WatchStatus, WatchlistTypeFilter};

    let custom_id = &component.data.custom_id;
    let parts: Vec<&str> = custom_id.split(':').collect();
    if parts.len() != 6 {
        return Ok(());
    }

    let caller_id: u64 = parts[1].parse().unwrap_or(0);
    let target_id: u64 = parts[2].parse().unwrap_or(0);
    let page: usize = parts[3].parse().unwrap_or(1);
    let status_str = parts[4];
    let type_str = parts[5];

    let ctx = framework.serenity_context;
    let data = framework.user_data().await;

    // Enforce authorization: only caller who initiated /watchlist view can change pages
    if component.user.id.get() != caller_id {
        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::Message(
                    serenity::CreateInteractionResponseMessage::new()
                        .content("Only the user who ran this watchlist command can navigate pages.")
                        .ephemeral(true),
                ),
            )
            .await?;
        return Ok(());
    }

    // Resolve target user
    let target_user = if component.user.id.get() == target_id {
        component.user.clone()
    } else {
        match ctx.http.get_user(serenity::UserId::new(target_id)).await {
            Ok(u) => u,
            Err(_) => component.user.clone(),
        }
    };

    let status_filter = if status_str == "all" {
        None
    } else {
        Some(WatchStatus::from_db_str(status_str))
    };

    let type_filter = if type_str == "all" {
        None
    } else {
        Some(WatchlistTypeFilter::from_str(type_str))
    };

    let type_arg = type_filter.and_then(|t| t.as_db_str());
    let status_arg = status_filter.map(|s| s.as_db_str());

    let entries = data
        .db
        .get_unified_watchlist(target_id, type_arg, status_arg)
        .await?;

    let (embed, action_row) = render_watchlist_view(
        caller_id,
        &target_user,
        &entries,
        page,
        status_filter,
        type_filter,
    );

    let mut response_msg = serenity::CreateInteractionResponseMessage::new().embed(embed);
    if let Some(row) = action_row {
        response_msg = response_msg.components(vec![row]);
    } else {
        response_msg = response_msg.components(vec![]);
    }

    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(response_msg),
        )
        .await?;

    Ok(())
}


