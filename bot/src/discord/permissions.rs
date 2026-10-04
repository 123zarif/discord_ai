use crate::discord::{Context, Error};

/// Checks if the command invoker is the configured bot owner.
pub fn is_owner(ctx: Context<'_>) -> bool {
    ctx.data().config.is_owner(ctx.author().id)
}

/// Checks if the command invoker is the configured target user.
#[allow(dead_code)]
pub fn is_target_user(ctx: Context<'_>) -> bool {
    ctx.data().config.is_target_user(ctx.author().id)
}

/// Poise command check ensuring only the bot owner can execute the command.
///
/// Can be used on any command with `#[poise::command(slash_command, check = "crate::discord::permissions::require_owner")]`.
#[allow(dead_code)]
pub async fn require_owner(ctx: Context<'_>) -> Result<bool, Error> {
    if is_owner(ctx) {
        Ok(true)
    } else {
        ctx.send(
            poise::CreateReply::default()
                .content("❌ **Permission Denied**: This command is restricted to the bot owner.")
                .ephemeral(true),
        )
        .await?;
        Ok(false)
    }
}

/// Poise command check ensuring only the target user can execute the command.
#[allow(dead_code)]
pub async fn require_target_user(ctx: Context<'_>) -> Result<bool, Error> {
    if is_target_user(ctx) {
        Ok(true)
    } else {
        ctx.send(
            poise::CreateReply::default()
                .content("❌ **Permission Denied**: This command is restricted to the target user.")
                .ephemeral(true),
        )
        .await?;
        Ok(false)
    }
}
