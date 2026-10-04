use poise::serenity_prelude as serenity;
use serenity::model::id::UserId;
use crate::discord::actions::{self, ActionType};
use crate::discord::{Context, Error};

pub const KC_TARGET_USER_ID: u64 = 759834538448388206;

/// Easily send a sweet kiss to user 759834538448388206!
#[poise::command(slash_command)]
pub async fn kc(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let target_id = UserId::new(KC_TARGET_USER_ID);
    let author = ctx.author();
    let author_name = author.global_name.as_deref().unwrap_or(&author.name);
    let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or(0);

    // Resolve target username if accessible, otherwise mention
    let target_display = match target_id.to_user(&ctx).await {
        Ok(user) => user.global_name.unwrap_or(user.name),
        Err(_) => format!("<@{KC_TARGET_USER_ID}>"),
    };

    let media = actions::fetch_action_media(ActionType::Kiss).await;

    let is_self = author.id == target_id;
    let description = if is_self {
        format!("**{author_name}** kissed themselves... showing some self-love! 💕")
    } else {
        format!("**{author_name}** kissed **{target_display}**! 💕")
    };

    let count = ctx
        .data()
        .db
        .increment_action_count(
            guild_id,
            author.id.get(),
            target_id.get(),
            ActionType::Kiss.endpoint_name(),
        )
        .await
        .unwrap_or(1);

    let times_str = if count == 1 { "time" } else { "times" };
    let count_text = if is_self {
        format!("*(They've kissed themselves {} {})*", count, times_str)
    } else {
        format!("*({author_name} has kissed {target_display} {count} {times_str}!)*")
    };

    let full_description = format!("{description}\n\n{count_text}");

    let mut embed = serenity::CreateEmbed::new()
        .description(full_description)
        .image(media.url)
        .color(ActionType::Kiss.embed_color());

    if let Some(anime) = media.anime_name {
        embed = embed.footer(serenity::CreateEmbedFooter::new(format!("Anime: {anime}")));
    }

    let mut reply = poise::CreateReply::default().embed(embed);
    if !is_self {
        let row = actions::build_action_buttons(
            author.id.get(),
            target_id.get(),
            ActionType::Kiss,
        );
        reply = reply.components(vec![row]);
    }

    ctx.send(reply).await?;
    Ok(())
}
