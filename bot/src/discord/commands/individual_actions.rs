use poise::serenity_prelude as serenity;
use crate::discord::actions::{self, ActionType};
use crate::discord::{AppData, Context, Error};

/// Common execution handler for targeted/interactive actions with a required target user.
pub async fn execute_interactive_action(
    ctx: Context<'_>,
    action_type: ActionType,
    target: serenity::User,
) -> Result<(), Error> {
    ctx.defer().await?;

    let author = ctx.author();
    let author_name = author.global_name.as_deref().unwrap_or(&author.name);
    let target_name = target.global_name.as_deref().unwrap_or(&target.name);
    let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or(0);
    let is_self = target.id == author.id;

    let media = actions::fetch_action_media(action_type).await;

    let desc = if is_self {
        action_type.format_self(author_name)
    } else {
        action_type.format_targeted(author_name, target_name)
    };

    let count = ctx
        .data()
        .db
        .increment_action_count(
            guild_id,
            author.id.get(),
            target.id.get(),
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

    let full_description = format!("{desc}\n\n{count_str}");

    let mut embed = serenity::CreateEmbed::new()
        .description(full_description)
        .image(media.url)
        .color(action_type.embed_color());

    if let Some(anime) = media.anime_name {
        embed = embed.footer(serenity::CreateEmbedFooter::new(format!("Anime: {anime}")));
    }

    let mut reply = poise::CreateReply::default().embed(embed);
    if !is_self {
        let row = actions::build_action_buttons(
            author.id.get(),
            target.id.get(),
            action_type,
        );
        reply = reply.components(vec![row]);
    }

    ctx.send(reply).await?;
    Ok(())
}

/// Common execution handler for solo emotes.
pub async fn execute_solo_action(
    ctx: Context<'_>,
    action_type: ActionType,
) -> Result<(), Error> {
    ctx.defer().await?;

    let author = ctx.author();
    let author_name = author.global_name.as_deref().unwrap_or(&author.name);

    let media = actions::fetch_action_media(action_type).await;
    let description = action_type.format_solo(author_name);

    let mut embed = serenity::CreateEmbed::new()
        .description(description)
        .image(media.url)
        .color(action_type.embed_color());

    if let Some(anime) = media.anime_name {
        embed = embed.footer(serenity::CreateEmbedFooter::new(format!("Anime: {anime}")));
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

macro_rules! interactive_action_cmd {
    ($fn_name:ident, $action_variant:ident, $desc:literal) => {
        #[doc = $desc]
        #[poise::command(slash_command)]
        pub async fn $fn_name(
            ctx: Context<'_>,
            #[description = "Target user for the action"] target: serenity::User,
        ) -> Result<(), Error> {
            execute_interactive_action(ctx, ActionType::$action_variant, target).await
        }
    };
}

macro_rules! solo_action_cmd {
    ($fn_name:ident, $action_variant:ident, $desc:literal) => {
        #[doc = $desc]
        #[poise::command(slash_command)]
        pub async fn $fn_name(ctx: Context<'_>) -> Result<(), Error> {
            execute_solo_action(ctx, ActionType::$action_variant).await
        }
    };
}

// 20 Interactive actions with required target
interactive_action_cmd!(kiss, Kiss, "Kiss someone sweetly! 💕");
interactive_action_cmd!(hug, Hug, "Give someone a warm hug! 🤗");
interactive_action_cmd!(pat, Pat, "Gently pat someone on the head! ✨");
interactive_action_cmd!(slap, Slap, "Slap someone! Ouch! 💥");
interactive_action_cmd!(cuddle, Cuddle, "Cuddle up with someone! 🥰");
interactive_action_cmd!(bite, Bite, "Bite someone! Nom! 🦷");
interactive_action_cmd!(poke, Poke, "Poke someone! 👉");
interactive_action_cmd!(tickle, Tickle, "Tickle someone! Hehe! 😆");
interactive_action_cmd!(feed, Feed, "Feed someone tasty food! 🍰");
interactive_action_cmd!(handhold, Handhold, "Hold hands with someone! 🤝");
interactive_action_cmd!(highfive, Highfive, "High-five someone! 🙏");
interactive_action_cmd!(kick, Kick, "Kick someone! 🦵");
interactive_action_cmd!(punch, Punch, "Punch someone! 👊");
interactive_action_cmd!(shoot, Shoot, "Shoot someone! Bang! 💥");
interactive_action_cmd!(stare, Stare, "Stare intently at someone... 👀");
interactive_action_cmd!(wave, Wave, "Wave at someone! 👋");
interactive_action_cmd!(wink, Wink, "Wink at someone! 😉");
interactive_action_cmd!(clap, Clap, "Clap for someone! 👏");
interactive_action_cmd!(nom, Nom, "Nibble on someone! 🍙");
interactive_action_cmd!(smile, Smile, "Smile warmly at someone! 😊");

// 14 Solo emotes
solo_action_cmd!(blush, Blush, "Blush furiously! 😳");
solo_action_cmd!(cry, Cry, "Cry sad tears... 😭");
solo_action_cmd!(dance, Dance, "Dance joyfully! 💃");
solo_action_cmd!(happy, Happy, "Express pure happiness! 🥳");
solo_action_cmd!(laugh, Laugh, "Burst out laughing! 🤣");
solo_action_cmd!(nod, Nod, "Nod in agreement! ✅");
solo_action_cmd!(nope, Nope, "Say nope! Absolutely not! 🙅");
solo_action_cmd!(pout, Pout, "Pout unhappily! 😾");
solo_action_cmd!(shrug, Shrug, "Shrug nonchalantly! ¯\\_(ツ)_/¯");
solo_action_cmd!(sleep, Sleep, "Fall asleep peacefully... Zzz 💤");
solo_action_cmd!(smug, Smug, "Wear a smug grin! 😏");
solo_action_cmd!(think, Think, "Think deeply... 🤔");
solo_action_cmd!(thumbsup, Thumbsup, "Give a big thumbs up! 👍");
solo_action_cmd!(yawn, Yawn, "Yawn sleepily... 🥱");

/// Returns all 34 individual action slash commands.
pub fn all_action_commands() -> Vec<poise::Command<AppData, Error>> {
    vec![
        // Interactive
        kiss(),
        hug(),
        pat(),
        slap(),
        cuddle(),
        bite(),
        poke(),
        tickle(),
        feed(),
        handhold(),
        highfive(),
        kick(),
        punch(),
        shoot(),
        stare(),
        wave(),
        wink(),
        clap(),
        nom(),
        smile(),
        // Solo emotes
        blush(),
        cry(),
        dance(),
        happy(),
        laugh(),
        nod(),
        nope(),
        pout(),
        shrug(),
        sleep(),
        smug(),
        think(),
        thumbsup(),
        yawn(),
    ]
}
