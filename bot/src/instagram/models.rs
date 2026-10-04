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

#[derive(Debug, Deserialize, Clone)]
pub struct MetaWebhookPayload {
    pub object: Option<String>,
    #[serde(default)]
    pub entry: Vec<MetaEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetaEntry {
    pub id: Option<String>,
    pub time: Option<i64>,
    #[serde(default)]
    pub messaging: Vec<MetaMessagingEvent>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetaMessagingEvent {
    pub sender: Option<MetaEntity>,
    pub recipient: Option<MetaEntity>,
    pub timestamp: Option<i64>,
    pub message: Option<MetaMessage>,
    pub share: Option<MetaShare>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetaEntity {
    pub id: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetaMessage {
    pub mid: Option<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub attachments: Vec<MetaAttachment>,
    pub share: Option<MetaShare>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetaAttachment {
    #[serde(rename = "type")]
    pub attachment_type: Option<String>,
    pub payload: Option<MetaAttachmentPayload>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetaAttachmentPayload {
    pub url: Option<String>,
    pub title: Option<String>,
    pub ig_post_media_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetaShare {
    pub link: Option<String>,
    pub id: Option<String>,
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
        let lower = url.to_lowercase();
        if lower.contains("/reel/") || lower.contains("/reels/") {
            Self::Reel
        } else if lower.contains("/collection") || lower.contains("/collections/") {
            Self::Collection
        } else if lower.contains("/p/") {
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

#[derive(Debug, Clone)]
pub enum InboundInstagramEvent {
    Media(ExtractedMedia),
    Text {
        sender_id: String,
        text: String,
    },
}

impl MetaWebhookPayload {
    /// Extracts all Instagram media (reels, posts, collections) or text messages.
    pub fn extract_events(&self) -> Vec<InboundInstagramEvent> {
        let mut results = Vec::new();

        for entry in &self.entry {
            for messaging in &entry.messaging {
                let sender_id = messaging
                    .sender
                    .as_ref()
                    .and_then(|s| s.id.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                let mut found_url: Option<String> = None;
                let mut user_comment: Option<String> = None;

                if let Some(ref msg) = messaging.message {
                    // 1. Check message text for any Instagram URL
                    if let Some(ref text) = msg.text {
                        for part in text.split_whitespace() {
                            let clean = part.trim_matches(|c| {
                                c == '<' || c == '>' || c == '"' || c == '\'' || c == '(' || c == ')'
                            });
                            if is_instagram_url(clean) {
                                found_url = Some(clean.to_string());
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
                                        || att_type == "share"
                                        || att_type == "ig_reel"
                                        || att_type == "ig_post"
                                        || att_type == "reel"
                                        || att_type == "carousel"
                                        || att_type == "video"
                                        || att_type == "image"
                                    {
                                        found_url = Some(url.clone());
                                        if user_comment.is_none() {
                                            if let Some(ref text) = msg.text {
                                                let trimmed = text.trim();
                                                if !trimmed.is_empty() {
                                                    user_comment = Some(trimmed.to_string());
                                                }
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // 3. Check message.share or messaging.share
                    if found_url.is_none() {
                        if let Some(ref share) = msg.share {
                            if let Some(ref link) = share.link {
                                found_url = Some(link.clone());
                            }
                        }
                    }
                    if found_url.is_none() {
                        if let Some(ref share) = messaging.share {
                            if let Some(ref link) = share.link {
                                found_url = Some(link.clone());
                            }
                        }
                    }

                    if let Some(media_url) = found_url {
                        let media_kind = InstagramMediaKind::from_url(&media_url);
                        results.push(InboundInstagramEvent::Media(ExtractedMedia {
                            sender_id,
                            media_url,
                            user_text: user_comment,
                            media_kind,
                        }));
                    } else if let Some(ref text) = msg.text {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            results.push(InboundInstagramEvent::Text {
                                sender_id,
                                text: trimmed.to_string(),
                            });
                        }
                    }
                }
            }
        }

        // Correlate: if a sender has both a text event and a media event without caption in the same payload,
        // merge the text into the media event's caption so it posts as a single unified message.
        let mut final_events = Vec::new();
        let mut text_by_sender: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        for event in results {
            match event {
                InboundInstagramEvent::Text { sender_id, text } => {
                    text_by_sender.insert(sender_id, text);
                }
                InboundInstagramEvent::Media(mut media) => {
                    if media.user_text.is_none() {
                        if let Some(text) = text_by_sender.remove(&media.sender_id) {
                            media.user_text = Some(text);
                        }
                    }
                    final_events.push(InboundInstagramEvent::Media(media));
                }
            }
        }

        // Any remaining text events without accompanying media
        for (sender_id, text) in text_by_sender {
            final_events.push(InboundInstagramEvent::Text { sender_id, text });
        }

        final_events
    }

    /// Backwards compatibility helper for media-only extraction.
    pub fn extract_media(&self) -> Vec<ExtractedMedia> {
        self.extract_events()
            .into_iter()
            .filter_map(|e| match e {
                InboundInstagramEvent::Media(m) => Some(m),
                _ => None,
            })
            .collect()
    }
}

/// Checks whether a URL points to an Instagram reel, post, collection, or share.
pub fn is_instagram_url(s: &str) -> bool {
    let lower = s.to_lowercase();
    (lower.contains("instagram.com/") || lower.contains("instagr.am/"))
        && (lower.contains("/reel/")
            || lower.contains("/reels/")
            || lower.contains("/p/")
            || lower.contains("/share/")
            || lower.contains("/collection/")
            || lower.contains("/collections/")
            || lower.contains("/tv/")
            || lower.contains("/stories/"))
}
