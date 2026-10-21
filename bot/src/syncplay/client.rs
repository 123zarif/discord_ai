use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
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
    pub active_rooms: Vec<String>,
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

/// Resolves host strings with graceful fallback for host.docker.internal when running natively on Linux.
pub async fn resolve_host(host_str: &str) -> String {
    if host_str.starts_with("host.docker.internal") {
        if tokio::net::lookup_host(host_str).await.is_err() {
            return host_str.replace("host.docker.internal", "127.0.0.1");
        }
    }
    host_str.to_string()
}

/// Generates a compliant Syncplay worker username (max 15 characters).
fn generate_worker_username(base: &str, room: &str) -> String {
    let clean_room: String = room.chars().filter(|c| c.is_alphanumeric()).collect();
    let prefix = if base.len() > 6 { &base[..6] } else { base };
    let mut name = format!("{prefix}_{clean_room}");
    if name.len() > 15 {
        name.truncate(15);
    }
    name
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
        active_rooms: vec![config.room.clone()],
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
                    s.active_rooms = vec![config.room.clone()];
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

struct RoomWorkerHandle {
    chat_tx: mpsc::Sender<String>,
    stop_tx: Option<oneshot::Sender<()>>,
}

async fn connect_and_run(
    config: &SyncplayClientConfig,
    cmd_rx: &mut mpsc::Receiver<ClientCommand>,
    event_tx: &mpsc::Sender<BridgeEvent>,
    status_tx: &watch::Sender<SyncplayStatus>,
    is_connected: &Arc<AtomicBool>,
) -> anyhow::Result<ConnectionExit> {
    let resolved_host = resolve_host(&config.host).await;
    let stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(&resolved_host)).await??;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // 1. Send Hello handshake for primary room
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
        s.active_rooms = vec![config.room.clone()];
    });

    // Track all users across all rooms: username -> roomName
    let mut all_users: HashMap<String, String> = HashMap::new();
    let mut room_workers: HashMap<String, RoomWorkerHandle> = HashMap::new();
    let worker_prefix = if config.username.len() > 6 { &config.username[..6] } else { &config.username };

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
                                    if author != config.username && !author.starts_with(worker_prefix) {
                                        let _ = event_tx.send(BridgeEvent::Chat {
                                            username: author,
                                            room: config.room.clone(),
                                            message: chat_msg.message,
                                        }).await;
                                    }
                                }
                            }
                        }
                        SyncplayPacket::ListWrapper { list } => {
                            // Initial room / user inventory from server
                            if let Some(rooms_map) = list.as_object() {
                                for (r_name, users_val) in rooms_map {
                                    if let Some(users_map) = users_val.as_object() {
                                        for (u_name, _) in users_map {
                                            if u_name != &config.username && !u_name.starts_with(worker_prefix) {
                                                all_users.insert(u_name.clone(), r_name.clone());
                                            }
                                        }
                                    }
                                }
                                sync_room_workers(config, &all_users, &mut room_workers, event_tx, status_tx);
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

                            // Relay playback pause / unpause changes in primary room
                            if let Some(ps) = state.playstate {
                                if let Some(set_by) = ps.setBy {
                                    if set_by != config.username && !set_by.starts_with(worker_prefix) {
                                        status_tx.send_modify(|s| {
                                            s.is_paused = ps.paused;
                                        });

                                        let _ = event_tx.send(BridgeEvent::PlaystateChanged {
                                            room: config.room.clone(),
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
                                    if uname == &config.username || uname.starts_with(worker_prefix) {
                                        continue;
                                    }

                                    if let Ok(details) = serde_json::from_value::<UserDetails>(details_val.clone()) {
                                        let user_room = details
                                            .room
                                            .as_ref()
                                            .map(|r| r.name.clone())
                                            .unwrap_or_else(|| config.room.clone());

                                        if let Some(event) = details.event {
                                            if event.joined == Some(true) {
                                                all_users.insert(uname.clone(), user_room.clone());
                                                sync_room_workers(config, &all_users, &mut room_workers, event_tx, status_tx);

                                                let _ = event_tx.send(BridgeEvent::UserJoined {
                                                    username: uname.clone(),
                                                    room: user_room.clone(),
                                                }).await;
                                            } else if event.left == Some(true) {
                                                all_users.remove(uname);
                                                sync_room_workers(config, &all_users, &mut room_workers, event_tx, status_tx);

                                                let _ = event_tx.send(BridgeEvent::UserLeft {
                                                    username: uname.clone(),
                                                    room: user_room.clone(),
                                                }).await;
                                            }
                                        }

                                        if let Some(file) = details.file {
                                            status_tx.send_modify(|s| {
                                                s.current_file = Some(file.name.clone());
                                            });

                                            let _ = event_tx.send(BridgeEvent::FileLoaded {
                                                username: uname.clone(),
                                                room: user_room.clone(),
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
                        let payload = format!("{}\r\n", serde_json::to_string(&chat_packet)?);

                        // Broadcast to primary room
                        let _ = writer.write_all(payload.as_bytes()).await;
                        let _ = writer.flush().await;

                        // Broadcast to all active auxiliary room workers
                        for worker in room_workers.values() {
                            let _ = worker.chat_tx.send(msg.clone()).await;
                        }
                    }
                    Some(ClientCommand::SwitchRoom { room, password }) => {
                        // Stop all auxiliary workers before switching primary room
                        for mut worker in room_workers.drain().map(|(_, w)| w) {
                            if let Some(stop) = worker.stop_tx.take() {
                                let _ = stop.send(());
                            }
                        }

                        return Ok(ConnectionExit::RoomSwitched {
                            new_room: room,
                            new_password: password,
                        });
                    }
                    Some(ClientCommand::Disconnect) | None => {
                        for mut worker in room_workers.drain().map(|(_, w)| w) {
                            if let Some(stop) = worker.stop_tx.take() {
                                let _ = stop.send(());
                            }
                        }
                        return Ok(ConnectionExit::RequestedDisconnect);
                    }
                }
            }
        }
    }
}

/// Dynamically maintains room listeners for every room that contains active users.
fn sync_room_workers(
    config: &SyncplayClientConfig,
    all_users: &HashMap<String, String>,
    room_workers: &mut HashMap<String, RoomWorkerHandle>,
    event_tx: &mpsc::Sender<BridgeEvent>,
    status_tx: &watch::Sender<SyncplayStatus>,
) {
    let active_rooms_set: HashSet<String> = all_users.values().cloned().collect();

    // 1. Remove workers for rooms that no longer have any active users
    room_workers.retain(|room_name, worker| {
        if !active_rooms_set.contains(room_name) {
            if let Some(stop) = worker.stop_tx.take() {
                let _ = stop.send(());
            }
            false
        } else {
            true
        }
    });

    // 2. Spawn workers for any active room that is not the primary room and has no worker yet
    for room in &active_rooms_set {
        if room != &config.room && !room_workers.contains_key(room) {
            let (chat_tx, chat_rx) = mpsc::channel(50);
            let (stop_tx, stop_rx) = oneshot::channel();

            let worker_config = SyncplayClientConfig {
                host: config.host.clone(),
                username: generate_worker_username(&config.username, room),
                room: room.clone(),
                password: config.password.clone(),
            };

            let ev_tx = event_tx.clone();
            tokio::spawn(async move {
                run_room_worker(worker_config, chat_rx, stop_rx, ev_tx).await;
            });

            room_workers.insert(
                room.clone(),
                RoomWorkerHandle {
                    chat_tx,
                    stop_tx: Some(stop_tx),
                },
            );
        }
    }

    // 3. Update status reporting with all active rooms and viewer lists
    let mut rooms_list: Vec<String> = active_rooms_set.iter().cloned().collect();
    if !rooms_list.contains(&config.room) {
        rooms_list.push(config.room.clone());
    }
    rooms_list.sort();

    let mut users_list: Vec<String> = all_users
        .iter()
        .map(|(u, r)| format!("{u} (#{r})"))
        .collect();
    users_list.sort();

    status_tx.send_modify(|s| {
        s.active_rooms = rooms_list;
        s.active_users = users_list;
    });
}

/// Lightweight auxiliary worker listening to a secondary active room.
async fn run_room_worker(
    config: SyncplayClientConfig,
    mut chat_rx: mpsc::Receiver<String>,
    mut stop_rx: oneshot::Receiver<()>,
    event_tx: mpsc::Sender<BridgeEvent>,
) {
    let resolved_host = resolve_host(&config.host).await;
    let stream = match tokio::time::timeout(Duration::from_secs(8), TcpStream::connect(&resolved_host)).await {
        Ok(Ok(s)) => s,
        _ => return,
    };
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

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
                "readiness": true
            },
            "password": config.password
        }
    });

    if let Ok(data) = serde_json::to_string(&hello) {
        let _ = writer.write_all(format!("{data}\r\n").as_bytes()).await;
        let _ = writer.flush().await;
    }

    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                break;
            }
            line_res = lines.next_line() => {
                let line = match line_res {
                    Ok(Some(l)) => l,
                    _ => break,
                };
                let clean = line.trim();
                if clean.is_empty() {
                    continue;
                }

                if let Ok(packet) = serde_json::from_str::<SyncplayPacket>(clean) {
                    match packet {
                        SyncplayPacket::ChatWrapper { chat } => {
                            if let Ok(chat_msg) = serde_json::from_value::<ChatMessage>(chat) {
                                if let Some(author) = chat_msg.username {
                                    if author != config.username {
                                        let _ = event_tx.send(BridgeEvent::Chat {
                                            username: author,
                                            room: config.room.clone(),
                                            message: chat_msg.message,
                                        }).await;
                                    }
                                }
                            }
                        }
                        SyncplayPacket::StateWrapper { state } => {
                            // Respond to ping
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
                                    if let Ok(p_str) = serde_json::to_string(&pong) {
                                        let _ = writer.write_all(format!("{p_str}\r\n").as_bytes()).await;
                                        let _ = writer.flush().await;
                                    }
                                }
                            }

                            // Relay playstate in this room
                            if let Some(ps) = state.playstate {
                                if let Some(set_by) = ps.setBy {
                                    if set_by != config.username {
                                        let _ = event_tx.send(BridgeEvent::PlaystateChanged {
                                            room: config.room.clone(),
                                            paused: ps.paused,
                                            position: ps.position,
                                            set_by,
                                        }).await;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            chat_msg = chat_rx.recv() => {
                match chat_msg {
                    Some(msg) => {
                        let chat_packet = serde_json::json!({ "Chat": msg });
                        if let Ok(c_str) = serde_json::to_string(&chat_packet) {
                            let _ = writer.write_all(format!("{c_str}\r\n").as_bytes()).await;
                            let _ = writer.flush().await;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}
