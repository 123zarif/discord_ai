use poise::serenity_prelude as serenity;

pub const COLOR_ANILIST: u32 = 0x3DB4F2;
pub const COLOR_WATCHING: u32 = 0x3498DB;
pub const COLOR_COMPLETED: u32 = 0x2ECC71;
pub const COLOR_PLANNING: u32 = 0x9B59B6;
pub const COLOR_ON_HOLD: u32 = 0xF1C40F;
pub const COLOR_DROPPED: u32 = 0xE74C3C;
pub const COLOR_RECOMMEND: u32 = 0x8E44AD;

/// Returns an embed accent color corresponding to a watchlist status string.
pub fn status_color(status: &str) -> u32 {
    match status.to_lowercase().as_str() {
        "watching" => COLOR_WATCHING,
        "completed" => COLOR_COMPLETED,
        "planning" | "plan_to_watch" => COLOR_PLANNING,
        "on_hold" => COLOR_ON_HOLD,
        "dropped" => COLOR_DROPPED,
        _ => COLOR_ANILIST,
    }
}

/// Returns a modern badge format with emoji for a watchlist status.
pub fn status_badge(status: &str) -> &'static str {
    match status.to_lowercase().as_str() {
        "watching" => "📺 `Watching`",
        "completed" => "✅ `Completed`",
        "planning" | "plan_to_watch" => "📋 `Plan to Watch`",
        "on_hold" => "⏸️ `On Hold`",
        "dropped" => "🛑 `Dropped`",
        _ => "📁 `Watchlist`",
    }
}

/// Returns a clean human-readable format label (e.g. "TV", "Movie", "OVA").
pub fn clean_format(fmt: Option<&str>) -> String {
    match fmt {
        Some("TV") => "TV Series".to_string(),
        Some("TV_SHORT") => "TV Short".to_string(),
        Some("MOVIE") => "Movie".to_string(),
        Some("SPECIAL") => "Special".to_string(),
        Some("OVA") => "OVA".to_string(),
        Some("ONA") => "ONA".to_string(),
        Some("MUSIC") => "Music Video".to_string(),
        Some(other) => other.replace('_', " "),
        None => "Anime".to_string(),
    }
}

/// Returns a clean human-readable release status label.
pub fn clean_airing_status(status: Option<&str>) -> String {
    match status {
        Some("FINISHED") => "Finished".to_string(),
        Some("RELEASING") => "Airing".to_string(),
        Some("NOT_YET_RELEASED") => "Upcoming".to_string(),
        Some("CANCELLED") => "Cancelled".to_string(),
        Some("HIATUS") => "On Hiatus".to_string(),
        Some(other) => other.replace('_', " "),
        None => "Unknown".to_string(),
    }
}

/// Generates a sleek Unicode episode progress bar: `▰▰▰▰▱▱▱▱ 50%`.
pub fn make_progress_bar(current: i32, total: Option<i32>, length: usize) -> String {
    if let Some(total) = total {
        if total <= 0 {
            return format!("Ep {current}");
        }
        let clamped = current.max(0).min(total);
        let pct = ((clamped as f64 / total as f64) * 100.0).round() as i32;
        let filled = (((clamped as f64 / total as f64) * length as f64).round() as usize).min(length);
        let empty = length.saturating_sub(filled);
        format!("{}{} ({}%)", "▰".repeat(filled), "▱".repeat(empty), pct)
    } else if current > 0 {
        format!("Ep {current}")
    } else {
        String::new()
    }
}

/// Truncates text cleanly on word boundaries up to `max_chars`.
pub fn truncate_synopsis(synopsis: &str, max_chars: usize) -> (String, bool) {
    let clean = synopsis.trim();
    if clean.chars().count() <= max_chars {
        return (clean.to_string(), false);
    }

    let mut end_idx = 0;
    for (idx, ch) in clean.char_indices() {
        if idx > max_chars {
            break;
        }
        if ch.is_whitespace() {
            end_idx = idx;
        }
    }

    if end_idx == 0 {
        end_idx = max_chars.min(clean.len());
    }

    let truncated = clean[..end_idx].trim_end().to_string();
    (truncated, true)
}

/// Helper to format user avatar URL safely.
pub fn user_avatar_url(user: &serenity::User) -> String {
    user.avatar_url().unwrap_or_else(|| user.default_avatar_url())
}
