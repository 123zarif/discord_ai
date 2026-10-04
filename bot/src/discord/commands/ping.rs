use std::time::Instant;
use crate::discord::{Context, Error};

/// Health ping to verify bot responsiveness and latency.
#[poise::command(slash_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    let start = Instant::now();
    let reply = ctx.say("🏓 Pinging...").await?;
    let rtt_ms = start.elapsed().as_millis();

    let ws_latency = ctx.ping().await;

    reply
        .edit(
            ctx,
            poise::CreateReply::default().content(format!(
                "🏓 **Pong!**\n• Gateway Heartbeat: `{} ms`\n• Round-trip Latency: `{} ms`",
                ws_latency.as_millis(),
                rtt_ms
            )),
        )
        .await?;

    Ok(())
}
