use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionType {
    Kiss,
    Hug,
    Pat,
    Slap,
    Cuddle,
    Bite,
    Poke,
    Tickle,
    Feed,
    Handhold,
    Highfive,
    Kick,
    Punch,
    Shoot,
    Stare,
    Wave,
    Wink,
    Blush,
    Cry,
    Dance,
    Happy,
    Laugh,
    Nod,
    Nom,
    Nope,
    Pout,
    Shrug,
    Sleep,
    Smile,
    Smug,
    Think,
    Thumbsup,
    Yawn,
    Clap,
}

impl ActionType {
    pub const ALL: &'static [Self] = &[
        Self::Kiss,
        Self::Hug,
        Self::Pat,
        Self::Slap,
        Self::Cuddle,
        Self::Bite,
        Self::Poke,
        Self::Tickle,
        Self::Feed,
        Self::Handhold,
        Self::Highfive,
        Self::Kick,
        Self::Punch,
        Self::Shoot,
        Self::Stare,
        Self::Wave,
        Self::Wink,
        Self::Blush,
        Self::Cry,
        Self::Dance,
        Self::Happy,
        Self::Laugh,
        Self::Nod,
        Self::Nom,
        Self::Nope,
        Self::Pout,
        Self::Shrug,
        Self::Sleep,
        Self::Smile,
        Self::Smug,
        Self::Think,
        Self::Thumbsup,
        Self::Yawn,
        Self::Clap,
    ];

    /// Parses an action string (case-insensitive) into ActionType.
    pub fn from_str_case_insensitive(input: &str) -> Option<Self> {
        let clean = input.trim().to_lowercase();
        Self::ALL.iter().copied().find(|act| {
            act.endpoint_name() == clean || (act == &Self::Clap && clean == "clapping")
        })
    }
    /// Returns the nekos.best API endpoint category name.
    pub fn endpoint_name(&self) -> &'static str {
        match self {
            Self::Kiss => "kiss",
            Self::Hug => "hug",
            Self::Pat => "pat",
            Self::Slap => "slap",
            Self::Cuddle => "cuddle",
            Self::Bite => "bite",
            Self::Poke => "poke",
            Self::Tickle => "tickle",
            Self::Feed => "feed",
            Self::Handhold => "handhold",
            Self::Highfive => "highfive",
            Self::Kick => "kick",
            Self::Punch => "punch",
            Self::Shoot => "shoot",
            Self::Stare => "stare",
            Self::Wave => "wave",
            Self::Wink => "wink",
            Self::Blush => "blush",
            Self::Cry => "cry",
            Self::Dance => "dance",
            Self::Happy => "happy",
            Self::Laugh => "laugh",
            Self::Nod => "nod",
            Self::Nom => "nom",
            Self::Nope => "nope",
            Self::Pout => "pout",
            Self::Shrug => "shrug",
            Self::Sleep => "sleep",
            Self::Smile => "smile",
            Self::Smug => "smug",
            Self::Think => "think",
            Self::Thumbsup => "thumbsup",
            Self::Yawn => "yawn",
            Self::Clap => "clap",
        }
    }

    /// Past tense representation of the action for counter messages.
    pub fn past_tense(&self) -> &'static str {
        match self {
            Self::Kiss => "kissed",
            Self::Hug => "hugged",
            Self::Pat => "patted",
            Self::Slap => "slapped",
            Self::Cuddle => "cuddled",
            Self::Bite => "bit",
            Self::Poke => "poked",
            Self::Tickle => "tickled",
            Self::Feed => "fed",
            Self::Handhold => "held hands with",
            Self::Highfive => "high-fived",
            Self::Kick => "kicked",
            Self::Punch => "punched",
            Self::Shoot => "shot",
            Self::Stare => "stared at",
            Self::Wave => "waved at",
            Self::Wink => "winked at",
            Self::Clap => "clapped for",
            Self::Nom => "nibbled on",
            Self::Smile => "smiled at",
            _ => self.endpoint_name(),
        }
    }

    /// Label text for responding to an action (e.g. "Kiss Back", "Hug Back").
    pub fn action_back_label(&self) -> String {
        match self {
            Self::Handhold => "Hold Hands Back".to_string(),
            Self::Highfive => "High-Five Back".to_string(),
            Self::Nom => "Nibble Back".to_string(),
            _ => {
                let name = self.endpoint_name();
                let mut chars = name.chars();
                let capitalized = match chars.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                };
                format!("{capitalized} Back")
            }
        }
    }

    /// Returns true if this action is performed mutually/together (e.g. holding hands, highfive, cuddle, dance),
    /// meaning a response should NOT increment the counter an additional time.
    pub fn is_done_together(&self) -> bool {
        matches!(
            self,
            Self::Handhold | Self::Highfive | Self::Cuddle | Self::Dance
        )
    }

    /// Formats the action description when targeting another user.
    pub fn format_targeted(&self, author: &str, target: &str) -> String {
        match self {
            Self::Kiss => format!("**{author}** kissed **{target}**! 💕"),
            Self::Hug => format!("**{author}** gave **{target}** a warm hug! 🤗"),
            Self::Pat => format!("**{author}** gently patted **{target}** on the head! ✨"),
            Self::Slap => format!("**{author}** slapped **{target}**! Ouch! 💥"),
            Self::Cuddle => format!("**{author}** cuddled up with **{target}**! 🥰"),
            Self::Bite => format!("**{author}** bit **{target}**! Nom! 🦷"),
            Self::Poke => format!("**{author}** poked **{target}**! 👉"),
            Self::Tickle => format!("**{author}** tickled **{target}**! Hehe! 😆"),
            Self::Feed => format!("**{author}** fed **{target}**! Say aah! 🍰"),
            Self::Handhold => format!("**{author}** held hands with **{target}**! 🤝"),
            Self::Highfive => format!("**{author}** high-fived **{target}**! 🙏"),
            Self::Kick => format!("**{author}** kicked **{target}**! 🦵"),
            Self::Punch => format!("**{author}** punched **{target}**! 👊"),
            Self::Shoot => format!("**{author}** shot **{target}**! Bang! 💥"),
            Self::Stare => format!("**{author}** is staring intently at **{target}**... 👀"),
            Self::Wave => format!("**{author}** waved at **{target}**! 👋"),
            Self::Wink => format!("**{author}** winked at **{target}**! 😉"),
            Self::Clap => format!("**{author}** is clapping for **{target}**! 👏"),
            Self::Nom => format!("**{author}** is nibbling on **{target}**! 🍙"),
            Self::Smile => format!("**{author}** smiled sweetly at **{target}**! 😊"),
            _ => format!("**{author}** did **{}** towards **{target}**!", self.endpoint_name()),
        }
    }

    /// Formats the action description when targeting oneself.
    pub fn format_self(&self, author: &str) -> String {
        match self {
            Self::Kiss => format!("**{author}** kissed themselves... showing some self-love! 💕"),
            Self::Hug => format!("**{author}** hugged themselves tightly! 🤗"),
            Self::Pat => format!("**{author}** patted their own head... good job! ✨"),
            Self::Slap => format!("**{author}** slapped themselves! Wake up! 💥"),
            Self::Cuddle => format!("**{author}** wrapped themselves in a blanket! 🛋️"),
            Self::Bite => format!("**{author}** bit their own tongue! Ouch! 😖"),
            _ => self.format_solo(author),
        }
    }

    /// Formats the action description when invoked solo (no target specified).
    pub fn format_solo(&self, author: &str) -> String {
        match self {
            Self::Blush => format!("**{author}** is blushing furiously! 😳"),
            Self::Cry => format!("**{author}** is crying... someone give them a hug! 😭"),
            Self::Dance => format!("**{author}** started dancing joyfully! 💃"),
            Self::Happy => format!("**{author}** is feeling super happy! 🥳"),
            Self::Laugh => format!("**{author}** burst out laughing! 🤣"),
            Self::Nod => format!("**{author}** nodded in agreement! ✅"),
            Self::Nope => format!("**{author}** said: Nope! Absolutely not! 🙅"),
            Self::Pout => format!("**{author}** is pouting! Hmph! 😾"),
            Self::Shrug => format!("**{author}** shrugged! ¯\\_(ツ)_/¯"),
            Self::Sleep => format!("**{author}** fell asleep... Zzz 💤"),
            Self::Smile => format!("**{author}** is smiling brightly! 😊"),
            Self::Smug => format!("**{author}** is wearing a very smug grin! 😏"),
            Self::Think => format!("**{author}** is deep in thought... 🤔"),
            Self::Thumbsup => format!("**{author}** gave a big thumbs up! 👍"),
            Self::Yawn => format!("**{author}** yawned sleepily... 🥱"),
            Self::Clap => format!("**{author}** started clapping! 👏"),
            Self::Wave => format!("**{author}** waved to everyone! 👋"),
            _ => format!("**{author}** is feeling {}! ✨", self.endpoint_name()),
        }
    }

    /// Primary embed color associated with the action theme.
    pub fn embed_color(&self) -> u32 {
        match self {
            Self::Kiss | Self::Cuddle | Self::Handhold => 0xFF69B4, // Hot Pink
            Self::Hug | Self::Pat | Self::Smile => 0xFFA07A,      // Light Salmon / Warm
            Self::Slap | Self::Punch | Self::Kick => 0xDC143C,     // Crimson Red
            Self::Cry | Self::Sleep => 0x6495ED,                   // Cornflower Blue
            Self::Dance | Self::Happy | Self::Highfive => 0xFFD700,// Gold / Sunny
            _ => 0x9B59B6,                                         // Purple
        }
    }
}

/// Builds interactive response buttons ("<Action> Back" and "Reject") for targeted actions.
/// Buttons are text-only (no emojis).
pub fn build_action_buttons(
    author_id: u64,
    target_id: u64,
    action: ActionType,
) -> serenity::all::CreateActionRow {
    use poise::serenity_prelude as serenity;

    let back_id = format!("action:back:{}:{}:{}", author_id, target_id, action.endpoint_name());
    let reject_id = format!("action:reject:{}:{}:{}", author_id, target_id, action.endpoint_name());

    let back_button = serenity::CreateButton::new(back_id)
        .label(action.action_back_label())
        .style(serenity::ButtonStyle::Primary);

    let reject_button = serenity::CreateButton::new(reject_id)
        .label("Reject")
        .style(serenity::ButtonStyle::Danger);

    serenity::CreateActionRow::Buttons(vec![back_button, reject_button])
}

#[derive(Debug, Clone)]
pub struct ActionMedia {
    pub url: String,
    pub anime_name: Option<String>,
}

/// Fetches dynamic anime GIF media for a given action using the official `nekosbest` crate.
pub async fn fetch_action_media(action: ActionType) -> ActionMedia {
    let endpoint = action.endpoint_name();

    if let Some(category) = nekosbest::Category::from_url_name(endpoint) {
        match nekosbest::get(category).await {
            Ok(resp) => {
                let anime_name = match resp.details {
                    nekosbest::details::Details::Gif(gif) => Some(gif.anime_name),
                    _ => None,
                };
                return ActionMedia {
                    url: resp.url,
                    anime_name,
                };
            }
            Err(e) => {
                warn!("nekosbest client request failed for '{endpoint}': {e}");
            }
        }
    }

    warn!("Using verified fallback media for '{endpoint}'");
    fallback_media(action)
}

fn fallback_media(action: ActionType) -> ActionMedia {
    let (url, anime_name) = match action {
        ActionType::Kiss => (
            "https://nekos.best/api/v2/kiss/fee74a4b-4ca4-461d-b279-685291430258.gif",
            Some("Kin-iro Mosaic".to_string()),
        ),
        ActionType::Hug => (
            "https://nekos.best/api/v2/hug/f45aad5e-e82f-4cac-b071-9b6f67752e0e.gif",
            Some("Cuckoo no Iinazuke".to_string()),
        ),
        ActionType::Pat => (
            "https://nekos.best/api/v2/pat/8d2cc8a6-eff4-4c54-8cc1-e705a1c6e75d.gif",
            Some("Kobayashi-san Chi no Maid Dragon".to_string()),
        ),
        ActionType::Slap => (
            "https://nekos.best/api/v2/slap/2ae21a07-8dc9-4744-a3b6-007c39e70f95.gif",
            Some("Yuru Yuri".to_string()),
        ),
        _ => (
            "https://nekos.best/api/v2/smile/81b99d06-d2dc-493b-9d91-8b3bd34a150c.gif",
            Some("Sewayaki Kitsune no Senko-san".to_string()),
        ),
    };

    ActionMedia {
        url: url.to_string(),
        anime_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_formatting() {
        let kiss = ActionType::Kiss;
        assert_eq!(kiss.endpoint_name(), "kiss");
        assert_eq!(kiss.past_tense(), "kissed");
        assert!(kiss.format_targeted("Alice", "Bob").contains("Alice"));
        assert!(kiss.format_targeted("Alice", "Bob").contains("Bob"));
        assert!(kiss.format_self("Alice").contains("self-love"));

        let dance = ActionType::Dance;
        assert_eq!(dance.endpoint_name(), "dance");
        assert!(dance.format_solo("Alice").contains("dancing"));
    }

    #[tokio::test]
    async fn test_fetch_action_media_returns_valid_url() {
        let media = fetch_action_media(ActionType::Kiss).await;
        assert!(media.url.starts_with("http"), "Media URL must be valid HTTP/HTTPS: {}", media.url);
    }

    #[test]
    fn test_is_done_together() {
        assert!(ActionType::Handhold.is_done_together());
        assert!(ActionType::Highfive.is_done_together());
        assert!(ActionType::Cuddle.is_done_together());
        assert!(ActionType::Dance.is_done_together());

        assert!(!ActionType::Kiss.is_done_together());
        assert!(!ActionType::Slap.is_done_together());
        assert!(!ActionType::Hug.is_done_together());
    }
}
