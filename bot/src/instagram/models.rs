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

#[derive(Debug, Clone)]
pub struct ExtractedReel {
    pub sender_id: String,
    pub reel_url: String,
}

impl MetaWebhookPayload {
    /// Extracts all Instagram reel URLs along with their sender IDs from the webhook payload.
    pub fn extract_reels(&self) -> Vec<ExtractedReel> {
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

                    // 1. Check message text for instagram.com/reel(s)/ link
                    if let Some(ref text) = msg.text {
                        if text.contains("instagram.com/reel/") || text.contains("instagram.com/reels/") {
                            for part in text.split_whitespace() {
                                if part.contains("instagram.com/reel/") || part.contains("instagram.com/reels/") {
                                    // Strip surrounding punctuation if present
                                    let clean = part.trim_matches(|c| c == '<' || c == '>' || c == '"' || c == '\'');
                                    found_url = Some(clean.to_string());
                                    break;
                                }
                            }
                        }
                    }

                    // 2. Check attachments for type == "ig_reel"
                    if found_url.is_none() {
                        for attachment in &msg.attachments {
                            if attachment.attachment_type.as_deref() == Some("ig_reel") {
                                if let Some(ref payload) = attachment.payload {
                                    if let Some(ref url) = payload.url {
                                        found_url = Some(url.clone());
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if let Some(reel_url) = found_url {
                        results.push(ExtractedReel {
                            sender_id,
                            reel_url,
                        });
                    }
                }
            }
        }

        results
    }
}
