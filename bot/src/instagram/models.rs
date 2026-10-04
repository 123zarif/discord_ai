use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MetaVerificationQuery {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MetaWebhookPayload {
    pub object: Option<String>,
    #[serde(default)]
    pub entry: Vec<MetaEntry>,
}

#[derive(Debug, Deserialize)]
pub struct MetaEntry {
    pub id: Option<String>,
    pub time: Option<i64>,
    #[serde(default)]
    pub messaging: Vec<MetaMessagingEvent>,
}

#[derive(Debug, Deserialize)]
pub struct MetaMessagingEvent {
    pub sender: Option<MetaEntity>,
    pub recipient: Option<MetaEntity>,
    pub timestamp: Option<i64>,
    pub message: Option<MetaMessage>,
}

#[derive(Debug, Deserialize)]
pub struct MetaEntity {
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MetaMessage {
    pub mid: Option<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub attachments: Vec<MetaAttachment>,
}

#[derive(Debug, Deserialize)]
pub struct MetaAttachment {
    #[serde(rename = "type")]
    pub attachment_type: Option<String>,
    pub payload: Option<MetaAttachmentPayload>,
}

#[derive(Debug, Deserialize)]
pub struct MetaAttachmentPayload {
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstagramMediaKind {
    Reel,
    Post,
    Collection,
    Generic,
}

impl InstagramMediaKind {
    pub fn from_url(url: &str) -> Self {
        if url.contains("/reel/") || url.contains("/reels/") {
            Self::Reel
        } else if url.contains("/collection") || url.contains("/collections/") {
            Self::Collection
        } else if url.contains("/p/") {
            Self::Post
        } else {
            Self::Generic
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Reel => "Instagram Reel",
            Self::Post => "Instagram Post",
            Self::Collection => "Instagram Collection",
            Self::Generic => "Instagram Share",
        }
    }

    pub fn button_label(&self) -> &'static str {
        match self {
            Self::Reel => "Watch Reel",
            Self::Post => "View Post",
            Self::Collection => "Open Collection",
            Self::Generic => "Open on Instagram",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedMedia {
    pub sender_id: String,
    pub media_url: String,
    pub user_text: Option<String>,
    pub media_kind: InstagramMediaKind,
}

impl MetaWebhookPayload {
    /// Extracts all Instagram media (reels, posts, collections) and optional accompanying text.
    pub fn extract_media(&self) -> Vec<ExtractedMedia> {
        let mut results = Vec::new();

        for entry in &self.entry {
            for messaging in &entry.messaging {
                let sender_id = messaging
                    .sender
                    .as_ref()
                    .and_then(|s| s.id.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                if let Some(ref msg) = messaging.message {
                    let mut found_url: Option<String> = None;
                    let mut user_comment: Option<String> = None;

                    // 1. Check message text for any Instagram URL
                    if let Some(ref text) = msg.text {
                        for part in text.split_whitespace() {
                            let clean = part.trim_matches(|c| c == '<' || c == '>' || c == '"' || c == '\'');
                            if is_instagram_url(clean) {
                                found_url = Some(clean.to_string());
                                // Extract the rest of the text as the user's caption/comment
                                let remaining = text.replace(clean, "");
                                let trimmed = remaining.trim();
                                if !trimmed.is_empty() {
                                    user_comment = Some(trimmed.to_string());
                                }
                                break;
                            }
                        }
                    }

                    // 2. Check attachments if URL wasn't embedded directly in text
                    if found_url.is_none() {
                        for attachment in &msg.attachments {
                            let att_type = attachment.attachment_type.as_deref().unwrap_or_default();
                            if let Some(ref payload) = attachment.payload {
                                if let Some(ref url) = payload.url {
                                    if is_instagram_url(url)
                                        || att_type == "ig_reel"
                                        || att_type == "share"
                                        || att_type == "carousel"
                                    {
                                        found_url = Some(url.clone());
                                        // When shared via Instagram DM share button, msg.text holds the user's comment
                                        if let Some(ref text) = msg.text {
                                            let trimmed = text.trim();
                                            if !trimmed.is_empty() {
                                                user_comment = Some(trimmed.to_string());
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    } else if user_comment.is_none() {
                        // In case text has extra comment
                        if let Some(ref text) = msg.text {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() && found_url.as_deref() != Some(trimmed) {
                                user_comment = Some(trimmed.to_string());
                            }
                        }
                    }

                    if let Some(media_url) = found_url {
                        let media_kind = InstagramMediaKind::from_url(&media_url);
                        results.push(ExtractedMedia {
                            sender_id,
                            media_url,
                            user_text: user_comment,
                            media_kind,
                        });
                    }
                }
            }
        }

        results
    }
}

/// Checks whether a URL points to an Instagram reel, post, collection, or share.
fn is_instagram_url(s: &str) -> bool {
    (s.contains("instagram.com/") || s.contains("instagr.am/"))
        && (s.contains("/reel/")
            || s.contains("/reels/")
            || s.contains("/p/")
            || s.contains("/share/")
            || s.contains("/collection/")
            || s.contains("/collections/")
            || s.contains("/tv/"))
}
