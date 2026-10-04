use crate::discord::{permissions, Context, Error};

/// Reports system status and infrastructure health for Discord, PostgreSQL, and pgvector.
#[poise::command(slash_command)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let db = &ctx.data().db;
    let config = &ctx.data().config;

    let db_health_result = db.health_check().await;
    let ws_latency = ctx.ping().await;

    let is_owner = permissions::is_owner(ctx);
    let mut pool_diagnostic = String::new();

    let (db_line, pgvector_line, is_healthy) = match db_health_result {
        Ok(report) => {
            let db_status = if let Some(latency) = report.latency_ms {
                format!("Connected ({} ms)", latency)
            } else {
                "Connected".to_string()
            };

            let pgv_status = if report.pgvector_available {
                if let Some(version) = report.pgvector_version {
                    format!("Available (v{})", version)
                } else {
                    "Available".to_string()
                }
            } else {
                "Unavailable (extension not installed)".to_string()
            };

            if is_owner {
                pool_diagnostic = format!(
                    " | Pool: {} active / {} total",
                    report.pool_size.saturating_sub(report.idle_connections),
                    report.pool_size
                );
            }

            let healthy = report.connected && report.pgvector_available;
            (db_status, pgv_status, healthy)
        }
        Err(err) => {
            (
                format!("Error ({err})"),
                "Unavailable (database error)".to_string(),
                false,
            )
        }
    };

    let discord_line = format!("Connected ({} ms)", ws_latency.as_millis());
    let bot_line = if is_healthy { "Healthy" } else { "Degraded" };

    let mut content = format!(
        "```text\n\
Discord:  {discord_line}\n\
Database: {db_line}\n\
pgvector: {pgvector_line}\n\
Bot:      {bot_line}\n\
```"
    );

    // Provide administrative diagnostic context if the caller is the bot owner
    if is_owner {
        content.push_str(&format!(
            "\n*Owner Diagnostics: Target User `{}` | Owner `{}`{}*",
            config.target_user_id, config.owner_id, pool_diagnostic
        ));
    }

    ctx.say(content).await?;
    Ok(())
}
