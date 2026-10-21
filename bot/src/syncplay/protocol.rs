#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

/// Client Hello message sent when connecting to Syncplay server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientHello {
    pub username: String,
    pub room: RoomName,
    pub version: String,
    pub realversion: Option<String>,
    pub features: ClientFeatures,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoomName {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientFeatures {
    pub chat: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managedRooms: Option<bool>,
}

impl Default for ClientFeatures {
    fn default() -> Self {
        Self {
            chat: true,
            readiness: Some(true),
            managedRooms: Some(true),
        }
    }
}

/// Outbound/Inbound Chat message payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

/// Ping packet for latency calculation and keepalive.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PingPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latencyCalculation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clientLatencyCalculation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serverRtt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clientRtt: Option<f64>,
}

/// Playstate payload detailing video playback position and paused state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaystatePayload {
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub paused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doSeek: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setBy: Option<String>,
}

/// State update containing ping and/or playback changes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ping: Option<PingPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playstate: Option<PlaystatePayload>,
}

/// Details about a media file currently loaded by a user.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserFile {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub duration: f64,
}

/// User event detailing joining or leaving.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserEvent {
    #[serde(default)]
    pub joined: Option<bool>,
    #[serde(default)]
    pub left: Option<bool>,
}

/// User entry inside Set messages.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<UserFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<UserEvent>,
}

/// High-level Syncplay message received from server or sent by client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SyncplayPacket {
    HelloWrapper {
        #[serde(rename = "Hello")]
        hello: serde_json::Value,
    },
    ChatWrapper {
        #[serde(rename = "Chat")]
        chat: serde_json::Value,
    },
    SetWrapper {
        #[serde(rename = "Set")]
        set: serde_json::Value,
    },
    ListWrapper {
        #[serde(rename = "List")]
        list: serde_json::Value,
    },
    StateWrapper {
        #[serde(rename = "State")]
        state: StatePayload,
    },
    Other(serde_json::Value),
}

/// Normalized event consumed by the bridge service.
#[derive(Debug, Clone)]
pub enum BridgeEvent {
    Connected {
        username: String,
        room: String,
        motd: Option<String>,
    },
    Chat {
        username: String,
        room: String,
        message: String,
    },
    UserJoined {
        username: String,
        room: String,
    },
    UserLeft {
        username: String,
        room: String,
    },
    FileLoaded {
        username: String,
        room: String,
        filename: String,
        duration: f64,
    },
    PlaystateChanged {
        room: String,
        paused: bool,
        position: f64,
        set_by: String,
    },
    Disconnected {
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chat_packet() {
        let raw = r#"{"Chat": {"message": "hello world", "username": "Alice"}}"#;
        let packet: SyncplayPacket = serde_json::from_str(raw).expect("should parse chat packet");
        match packet {
            SyncplayPacket::ChatWrapper { chat } => {
                let msg: ChatMessage = serde_json::from_value(chat).expect("should parse ChatMessage");
                assert_eq!(msg.username.as_deref(), Some("Alice"));
                assert_eq!(msg.message, "hello world");
            }
            _ => panic!("Expected ChatWrapper"),
        }
    }

    #[test]
    fn test_parse_ping_packet() {
        let raw = r#"{"State": {"ping": {"latencyCalculation": 12345.67, "serverRtt": 0}}}"#;
        let packet: SyncplayPacket = serde_json::from_str(raw).expect("should parse ping packet");
        match packet {
            SyncplayPacket::StateWrapper { state } => {
                let ping = state.ping.expect("ping should exist");
                assert_eq!(ping.latencyCalculation, Some(12345.67));
            }
            _ => panic!("Expected StateWrapper"),
        }
    }

    #[test]
    fn test_parse_list_packet() {
        let raw = r#"{"List": {"cinema": {"Alice": {"file": {"name": "movie.mkv"}}}}}"#;
        let packet: SyncplayPacket = serde_json::from_str(raw).expect("should parse list packet");
        match packet {
            SyncplayPacket::ListWrapper { list } => {
                assert!(list.get("cinema").is_some());
            }
            _ => panic!("Expected ListWrapper"),
        }
    }
}
