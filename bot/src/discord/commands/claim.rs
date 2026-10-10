use chrono::{Duration, Utc};
use poise::serenity_prelude as serenity;
use crate::db::NewDiscordMessage;
use crate::discord::{Context, Error};

/// Parses a timeframe string (e.g. "30m", "2h", "1d", "7d") into a chrono::Duration.
pub fn parse_timeframe(input: &str) -> Option<Duration> {
    let trimmed = input.trim().to_lowercase();
    if trimmed.is_empty() {
        return None;
    }

    let unit = trimmed.chars().last()?;
    let value_str = &trimmed[..trimmed.len() - 1];
    let value: i64 = value_str.parse().ok()?;

    if value <= 0 {
        return None;
    }

    match unit {
        'm' => Some(Duration::minutes(value)),
        'h' => Some(Duration::hours(value)),
        'd' => Some(Duration::days(value)),
        'w' => Some(Duration::weeks(value)),
        _ => None,
    }
}

/// Parses a message snowflake ID from either a raw string or a Discord message URL.
pub fn parse_message_id(input: &str) -> Option<u64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        // e.g. https://discord.com/channels/1234567890/9876543210/112233445566
        let path = trimmed.split('?').next().unwrap_or(trimmed);
        let last_segment = path.trim_end_matches('/').rsplit('/').next()?;
        last_segment.parse::<u64>().ok()
    } else {
        trimmed.parse::<u64>().ok()
    }
}

/// Backfills and claims chat history from this channel into PostgreSQL with pgvector embeddings.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("en-US", "Backfill and claim chat history from this channel into pgvector")
)]
pub async fn claim(
    ctx: Context<'_>,
    #[description = "Maximum messages to fetch (default: 100, max: 1000)"]
    limit: Option<u32>,
    #[description = "Timeframe window to fetch (e.g. 30m, 2h, 1d, 7d)"]
    timeframe: Option<String>,
    #[description = "Fetch messages older than this message ID or Discord message URL"]
    before: Option<String>,
    #[description = "Resume fetching backwards from the oldest message stored in the database"]
    continue_history: Option<bool>,
) -> Result<(), Error> {
    // Permission check: bot owner or guild administrator
    let is_owner = ctx.data().config.is_owner(ctx.author().id);
    let is_admin = match ctx.author_member().await {
        Some(member) => member
            .permissions
            .map(|p| p.contains(serenity::Permissions::ADMINISTRATOR))
            .unwrap_or(false),
        None => false,
    };

    if !is_owner && !is_admin {
        ctx.send(
            poise::CreateReply::default()
                .content("You do not have permission to use /claim. Only the bot owner or server administrators can claim channel data.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    // Parse timeframe if provided
    let cutoff_time = if let Some(ref tf) = timeframe {
        match parse_timeframe(tf) {
            Some(duration) => Some(Utc::now() - duration),
            None => {
                ctx.send(
                    poise::CreateReply::default()
                        .content("Invalid timeframe format. Use a number followed by m, h, d, or w (e.g. `30m`, `2h`, `1d`, `7d`).")
                        .ephemeral(true),
                )
                .await?;
                return Ok(());
            }
        }
    } else {
        None
    };

    let max_limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
    let channel_id = ctx.channel_id();

    // Determine initial pagination starting point (before_id)
    let mut before_id = if let Some(ref b) = before {
        match parse_message_id(b) {
            Some(id) => Some(serenity::MessageId::new(id)),
            None => {
                ctx.send(
                    poise::CreateReply::default()
                        .content("Invalid message ID or URL provided for `before`. Please provide a numeric snowflake ID or full Discord message link.")
                        .ephemeral(true),
                )
                .await?;
                return Ok(());
            }
        }
    } else if continue_history.unwrap_or(false) {
        ctx.data().db.get_oldest_message_id(channel_id.get()).await?.map(serenity::MessageId::new)
    } else {
        None
    };

    // Defer reply ephemerally as pagination and embedding may take several seconds
    ctx.defer_ephemeral().await?;

    let mut fetched_messages = Vec::new();

    while fetched_messages.len() < max_limit {
        let remaining = max_limit - fetched_messages.len();
        let batch_size = remaining.min(100) as u8;

        let mut builder = serenity::GetMessages::new().limit(batch_size);
        if let Some(bid) = before_id {
            builder = builder.before(bid);
        }

        let chunk = channel_id
            .messages(ctx.serenity_context(), builder)
            .await?;

        if chunk.is_empty() {
            break;
        }

        let mut reached_cutoff = false;
        for msg in &chunk {
            if let Some(cutoff) = cutoff_time {
                let msg_time = chrono::DateTime::<Utc>::from_timestamp_millis(
                    msg.timestamp.timestamp_millis(),
                )
                .unwrap_or_default();

                if msg_time < cutoff {
                    reached_cutoff = true;
                    break;
                }
            }

            fetched_messages.push(msg.clone());
            if fetched_messages.len() >= max_limit {
                break;
            }
        }

        if reached_cutoff {
            break;
        }

        before_id = chunk.last().map(|m| m.id);
    }

    // Filter out bot messages and empty messages
    let valid_messages: Vec<serenity::Message> = fetched_messages
        .into_iter()
        .filter(|m| !m.author.bot && !m.content.trim().is_empty())
        .collect();

    if valid_messages.is_empty() {
        ctx.send(
            poise::CreateReply::default().content("No human messages found matching the criteria in this channel."),
        )
        .await?;
        return Ok(());
    }

    // Check existing message IDs to deduplicate before computing embeddings
    let candidate_ids: Vec<u64> = valid_messages.iter().map(|m| m.id.get()).collect();
    let existing_ids = ctx.data().db.get_existing_message_ids(&candidate_ids).await?;

    let messages_to_embed: Vec<serenity::Message> = valid_messages
        .into_iter()
        .filter(|m| !existing_ids.contains(&m.id.get()))
        .collect();

    let duplicates_skipped = candidate_ids.len() - messages_to_embed.len();

    if messages_to_embed.is_empty() {
        let total_in_db = ctx
            .data()
            .db
            .count_channel_messages(channel_id.get())
            .await
            .unwrap_or(0);

        ctx.send(
            poise::CreateReply::default().content(format!(
                "All messages in this window have already been claimed (skipped {duplicates_skipped} duplicates).\nTotal messages stored for this channel: {total_in_db}"
            )),
        )
        .await?;
        return Ok(());
    }

    // Generate semantic vector embeddings for new messages
    let texts_for_embedding: Vec<String> = messages_to_embed
        .iter()
        .map(|m| format!("{}: {}", m.author.name, m.content))
        .collect();

    let embeddings = ctx
        .data()
        .embeddings
        .embed_batch(&texts_for_embedding)
        .await
        .map_err(|e| format!("Embedding generation failed: {e}"))?;

    let target_user_id = ctx.data().config.target_user_id.get();
    let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or(0);

    let mut db_records = Vec::with_capacity(messages_to_embed.len());
    let mut target_user_new_count = 0;

    for (msg, emb) in messages_to_embed.iter().zip(embeddings.into_iter()) {
        let is_target = msg.author.id.get() == target_user_id;
        if is_target {
            target_user_new_count += 1;
        }

        let timestamp = chrono::DateTime::<Utc>::from_timestamp_millis(
            msg.timestamp.timestamp_millis(),
        )
        .unwrap_or_else(Utc::now);

        let reply_to = msg.referenced_message.as_ref().map(|r| r.id.get());

        db_records.push(NewDiscordMessage {
            message_id: msg.id.get(),
            channel_id: channel_id.get(),
            guild_id,
            author_id: msg.author.id.get(),
            author_name: msg.author.global_name.clone().unwrap_or_else(|| msg.author.name.clone()),
            content: msg.content.clone(),
            is_target_user: is_target,
            reply_to_message_id: reply_to,
            message_timestamp: timestamp,
            embedding: Some(emb),
        });
    }

    let inserted = ctx
        .data()
        .db
        .batch_insert_messages(&db_records)
        .await
        .map_err(|e| format!("Database insertion failed: {e}"))?;

    let total_channel_count = ctx
        .data()
        .db
        .count_channel_messages(channel_id.get())
        .await
        .unwrap_or(inserted as i64);

    let total_target_user_count = ctx
        .data()
        .db
        .count_target_user_messages(channel_id.get())
        .await
        .unwrap_or(0);

    let timeframe_note = if let Some(ref tf) = timeframe {
        format!(" within timeframe `{tf}`")
    } else {
        String::new()
    };

    let start_point_note = if before.is_some() {
        " (starting before specified message)"
    } else if continue_history.unwrap_or(false) {
        " (continuing backwards from oldest stored message)"
    } else {
        ""
    };

    let reply_text = format!(
        "Claimed {inserted} new messages{timeframe_note}{start_point_note} (skipped {duplicates_skipped} duplicates).\nTarget user messages in this batch: {target_user_new_count} (Total target user messages in this channel: {total_target_user_count})\nTotal messages stored for this channel: {total_channel_count}"
    );

    ctx.send(poise::CreateReply::default().content(reply_text)).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timeframe_valid() {
        assert_eq!(parse_timeframe("30m"), Some(Duration::minutes(30)));
        assert_eq!(parse_timeframe("2h"), Some(Duration::hours(2)));
        assert_eq!(parse_timeframe("1d"), Some(Duration::days(1)));
        assert_eq!(parse_timeframe("2w"), Some(Duration::weeks(2)));
        assert_eq!(parse_timeframe(" 4h "), Some(Duration::hours(4)));
    }

    #[test]
    fn test_parse_timeframe_invalid() {
        assert_eq!(parse_timeframe(""), None);
        assert_eq!(parse_timeframe("0h"), None);
        assert_eq!(parse_timeframe("-5m"), None);
        assert_eq!(parse_timeframe("invalid"), None);
        assert_eq!(parse_timeframe("10x"), None);
    }

    #[test]
    fn test_parse_message_id_valid() {
        assert_eq!(parse_message_id("112233445566"), Some(112233445566));
        assert_eq!(
            parse_message_id("https://discord.com/channels/12345/67890/112233445566"),
            Some(112233445566)
        );
        assert_eq!(
            parse_message_id("https://discord.com/channels/12345/67890/112233445566?foo=bar"),
            Some(112233445566)
        );
        assert_eq!(
            parse_message_id("https://canary.discord.com/channels/12345/67890/99887766/"),
            Some(99887766)
        );
    }

    #[test]
    fn test_parse_message_id_invalid() {
        assert_eq!(parse_message_id(""), None);
        assert_eq!(parse_message_id("   "), None);
        assert_eq!(parse_message_id("not_a_number"), None);
        assert_eq!(parse_message_id("https://discord.com/"), None);
    }
}
