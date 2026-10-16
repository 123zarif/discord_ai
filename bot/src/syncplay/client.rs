use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use super::protocol::*;

/// Command sent to the client loop to interact with the Syncplay server.
#[derive(Debug)]
pub enum ClientCommand {
    SendChat(String),
    SwitchRoom {
        room: String,
        password: Option<String>,
    },
    Disconnect,
}

/// Client configuration for Syncplay connection.
#[derive(Debug, Clone)]
pub struct SyncplayClientConfig {
    pub host: String,
    pub username: String,
    pub room: String,
    pub password: Option<String>,
}

/// Active status of the Syncplay client.
#[derive(Debug, Clone, Default)]
pub struct SyncplayStatus {
    pub is_connected: bool,
    pub host: String,
    pub active_room: String,
    pub active_users: Vec<String>,
    pub current_file: Option<String>,
    pub is_paused: bool,
}

/// Handle to communicate with the background Syncplay client task.
#[derive(Clone)]
pub struct SyncplayClientHandle {
    cmd_tx: mpsc::Sender<ClientCommand>,
    status_rx: watch::Receiver<SyncplayStatus>,
    is_connected: Arc<AtomicBool>,
}

impl SyncplayClientHandle {
    pub async fn send_chat(&self, message: &str) -> anyhow::Result<()> {
        if !self.is_connected.load(Ordering::Relaxed) {
            anyhow::bail!("Syncplay client is not connected to server.");
        }
        self.cmd_tx
            .send(ClientCommand::SendChat(message.to_string()))
            .await
            .map_err(|_| anyhow::anyhow!("Syncplay command channel closed"))?;
        Ok(())
    }

    pub async fn switch_room(&self, room: &str, password: Option<&str>) -> anyhow::Result<()> {
        self.cmd_tx
            .send(ClientCommand::SwitchRoom {
                room: room.to_string(),
                password: password.map(|s| s.to_string()),
            })
            .await
            .map_err(|_| anyhow::anyhow!("Syncplay command channel closed"))?;
        Ok(())
    }

    pub fn get_status(&self) -> SyncplayStatus {
        self.status_rx.borrow().clone()
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }
}

/// Starts the Syncplay client with an event loop that maintains automatic reconnection.
pub fn start_syncplay_client(
    config: SyncplayClientConfig,
    event_tx: mpsc::Sender<BridgeEvent>,
) -> SyncplayClientHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel(100);
    let (status_tx, status_rx) = watch::channel(SyncplayStatus {
        is_connected: false,
        host: config.host.clone(),
        active_room: config.room.clone(),
        active_users: Vec::new(),
        current_file: None,
        is_paused: false,
    });
    let is_connected = Arc::new(AtomicBool::new(false));

    let handle = SyncplayClientHandle {
        cmd_tx,
        status_rx,
        is_connected: is_connected.clone(),
    };

    tokio::spawn(run_client_lifecycle(config, cmd_rx, event_tx, status_tx, is_connected));

    handle
}

async fn run_client_lifecycle(
    mut config: SyncplayClientConfig,
    mut cmd_rx: mpsc::Receiver<ClientCommand>,
    event_tx: mpsc::Sender<BridgeEvent>,
    status_tx: watch::Sender<SyncplayStatus>,
    is_connected: Arc<AtomicBool>,
) {
    let mut backoff = Duration::from_secs(2);

    loop {
        info!("Connecting to Syncplay server at {}...", config.host);

        match connect_and_run(&config, &mut cmd_rx, &event_tx, &status_tx, &is_connected).await {
            Ok(ConnectionExit::RequestedDisconnect) => {
                info!("Syncplay client requested disconnection.");
                is_connected.store(false, Ordering::Relaxed);
                break;
            }
            Ok(ConnectionExit::RoomSwitched { new_room, new_password }) => {
                info!("Switching Syncplay room to '{}'...", new_room);
                config.room = new_room;
                config.password = new_password;
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                warn!("Syncplay connection disconnected or failed: {e}. Retrying in {backoff:?}...");
                let _ = event_tx
                    .send(BridgeEvent::Disconnected {
                        reason: e.to_string(),
                    })
                    .await;

                is_connected.store(false, Ordering::Relaxed);
                status_tx.send_modify(|s| {
                    s.is_connected = false;
                    s.active_users.clear();
                });

                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {
                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(30));
                    }
                    cmd = cmd_rx.recv() => {
                        if let Some(ClientCommand::SwitchRoom { room, password }) = cmd {
                            config.room = room;
                            config.password = password;
                            backoff = Duration::from_secs(1);
                        } else if let Some(ClientCommand::Disconnect) = cmd {
                            break;
                        }
                    }
                }
            }
        }
    }
}

enum ConnectionExit {
    RequestedDisconnect,
    RoomSwitched {
        new_room: String,
        new_password: Option<String>,
    },
}

async fn connect_and_run(
    config: &SyncplayClientConfig,
    cmd_rx: &mut mpsc::Receiver<ClientCommand>,
    event_tx: &mpsc::Sender<BridgeEvent>,
    status_tx: &watch::Sender<SyncplayStatus>,
    is_connected: &Arc<AtomicBool>,
) -> anyhow::Result<ConnectionExit> {
    let stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(&config.host)).await??;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // 1. Send Hello handshake
    let hello = serde_json::json!({
        "Hello": {
            "username": config.username,
            "room": {
                "name": config.room
            },
            "version": "1.7.0",
            "realversion": "1.7.6",
            "features": {
                "chat": true,
                "readiness": true,
                "managedRooms": true
            },
            "password": config.password
        }
    });

    writer.write_all(format!("{}\r\n", serde_json::to_string(&hello)?).as_bytes()).await?;
    writer.flush().await?;

    is_connected.store(true, Ordering::Relaxed);
    status_tx.send_modify(|s| {
        s.is_connected = true;
        s.host = config.host.clone();
        s.active_room = config.room.clone();
    });

    let mut room_users: HashMap<String, ()> = HashMap::new();

    loop {
        tokio::select! {
            line_result = lines.next_line() => {
                let line = match line_result? {
                    Some(l) => l,
                    None => anyhow::bail!("Syncplay connection closed by remote server"),
                };

                let clean = line.trim();
                if clean.is_empty() {
                    continue;
                }

                if let Ok(packet) = serde_json::from_str::<SyncplayPacket>(clean) {
                    match packet {
                        SyncplayPacket::HelloWrapper { hello } => {
                            let motd = hello.get("motd").and_then(|v| v.as_str()).map(|s| s.to_string());
                            let _ = event_tx.send(BridgeEvent::Connected {
                                username: config.username.clone(),
                                room: config.room.clone(),
                                motd,
                            }).await;
                        }
                        SyncplayPacket::ChatWrapper { chat } => {
                            if let Ok(chat_msg) = serde_json::from_value::<ChatMessage>(chat) {
                                if let Some(author) = chat_msg.username {
                                    if author != config.username {
                                        let _ = event_tx.send(BridgeEvent::Chat {
                                            username: author,
                                            message: chat_msg.message,
                                        }).await;
                                    }
                                }
                            }
                        }
                        SyncplayPacket::StateWrapper { state } => {
                            // Respond to ping keepalives
                            if let Some(ping) = state.ping {
                                if let Some(calc) = ping.latencyCalculation {
                                    let pong = serde_json::json!({
                                        "State": {
                                            "ping": {
                                                "clientLatencyCalculation": calc,
                                                "clientRtt": 0.0
                                            }
                                        }
                                    });
                                    writer.write_all(format!("{}\r\n", serde_json::to_string(&pong)?).as_bytes()).await?;
                                    writer.flush().await?;
                                }
                            }

                            // Relay playback pause / unpause changes
                            if let Some(ps) = state.playstate {
                                if let Some(set_by) = ps.setBy {
                                    if set_by != config.username {
                                        status_tx.send_modify(|s| {
                                            s.is_paused = ps.paused;
                                        });

                                        let _ = event_tx.send(BridgeEvent::PlaystateChanged {
                                            paused: ps.paused,
                                            position: ps.position,
                                            set_by,
                                        }).await;
                                    }
                                }
                            }
                        }
                        SyncplayPacket::SetWrapper { set } => {
                            if let Some(user_obj) = set.get("user").and_then(|u| u.as_object()) {
                                for (uname, details_val) in user_obj {
                                    if uname == &config.username {
                                        continue;
                                    }

                                    if let Ok(details) = serde_json::from_value::<UserDetails>(details_val.clone()) {
                                        if let Some(event) = details.event {
                                            if event.joined == Some(true) {
                                                room_users.insert(uname.clone(), ());
                                                status_tx.send_modify(|s| {
                                                    s.active_users = room_users.keys().cloned().collect();
                                                });

                                                let _ = event_tx.send(BridgeEvent::UserJoined {
                                                    username: uname.clone(),
                                                    room: config.room.clone(),
                                                }).await;
                                            } else if event.left == Some(true) {
                                                room_users.remove(uname);
                                                status_tx.send_modify(|s| {
                                                    s.active_users = room_users.keys().cloned().collect();
                                                });

                                                let _ = event_tx.send(BridgeEvent::UserLeft {
                                                    username: uname.clone(),
                                                    room: config.room.clone(),
                                                }).await;
                                            }
                                        }

                                        if let Some(file) = details.file {
                                            status_tx.send_modify(|s| {
                                                s.current_file = Some(file.name.clone());
                                            });

                                            let _ = event_tx.send(BridgeEvent::FileLoaded {
                                                username: uname.clone(),
                                                filename: file.name,
                                                duration: file.duration,
                                            }).await;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(ClientCommand::SendChat(msg)) => {
                        let chat_packet = serde_json::json!({
                            "Chat": msg
                        });
                        writer.write_all(format!("{}\r\n", serde_json::to_string(&chat_packet)?).as_bytes()).await?;
                        writer.flush().await?;
                    }
                    Some(ClientCommand::SwitchRoom { room, password }) => {
                        return Ok(ConnectionExit::RoomSwitched {
                            new_room: room,
                            new_password: password,
                        });
                    }
                    Some(ClientCommand::Disconnect) => {
                        return Ok(ConnectionExit::RequestedDisconnect);
                    }
                    None => {
                        return Ok(ConnectionExit::RequestedDisconnect);
                    }
                }
            }
        }
    }
}
