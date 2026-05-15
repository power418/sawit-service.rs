use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tracing::{debug, info, warn};

use crate::{
    protocol::{
        ClientHello, ClientPacket, InputCommand, PROTOCOL_VERSION, PlayerFlags, PlayerId,
        PlayerSnapshot, Pong, RoomId, ServerError, ServerErrorCode, ServerPacket, Tick, Vec3,
        Welcome, WorldSnapshot, encode_server,
    },
    server::ServerConfig,
    simulation,
    wire::{ClientEncoding, ReceivedClientPacket, ServerResponse, encode_text_server},
};

#[derive(Debug, Default)]
pub(crate) struct RoomDirectory {
    rooms: HashMap<RoomId, RoomState>,
    addr_index: HashMap<SocketAddr, (RoomId, PlayerId)>,
    tick: Tick,
}

#[derive(Debug, Clone)]
pub(crate) struct RoomSummary {
    pub room_id: RoomId,
    pub player_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct DirectorySummary {
    pub tick: Tick,
    pub room_count: usize,
    pub player_count: usize,
    pub rooms: Vec<RoomSummary>,
}

#[derive(Debug, Default)]
struct RoomState {
    players: HashMap<PlayerId, PlayerState>,
}

#[derive(Debug)]
struct PlayerState {
    addr: SocketAddr,
    encoding: ClientEncoding,
    name: Option<String>,
    position: Vec3,
    velocity: Vec3,
    yaw: f32,
    pitch: f32,
    flags: PlayerFlags,
    last_processed_input: u32,
    last_input: Option<InputCommand>,
    last_seen: Instant,
}

impl PlayerState {
    fn snapshot(&self, player_id: PlayerId) -> PlayerSnapshot {
        PlayerSnapshot {
            player_id,
            position: self.position,
            velocity: self.velocity,
            yaw: self.yaw,
            pitch: self.pitch,
            flags: self.flags,
            last_processed_input: self.last_processed_input,
        }
    }
}

static NEXT_PLAYER_ID: AtomicU64 = AtomicU64::new(1);

impl RoomDirectory {
    pub(crate) fn handle_client_packet(
        &mut self,
        addr: SocketAddr,
        received: ReceivedClientPacket,
        config: &ServerConfig,
    ) -> Vec<ServerResponse> {
        let mut out = Vec::new();
        let ReceivedClientPacket { packet, encoding } = received;

        match packet {
            ClientPacket::Hello(hello) => {
                if hello.protocol_version != PROTOCOL_VERSION {
                    out.push(ServerResponse {
                        addr,
                        packet: ServerPacket::Error(ServerError {
                            code: ServerErrorCode::BadProtocolVersion,
                            message: format!(
                                "bad protocol_version={}, expected={}",
                                hello.protocol_version, PROTOCOL_VERSION
                            ),
                        }),
                        encoding,
                    });
                    return out;
                }

                let (room_id, player_id) = self.upsert_player(addr, hello, encoding, config);
                out.push(ServerResponse {
                    addr,
                    packet: ServerPacket::Welcome(Welcome {
                        protocol_version: PROTOCOL_VERSION,
                        player_id,
                        room_id,
                        tick_rate_hz: config.tick_rate_hz,
                        snapshot_rate_hz: config.snapshot_rate_hz,
                    }),
                    encoding,
                });
            }
            ClientPacket::Input(input) => {
                let Some((room_id, player_id)) = self.addr_index.get(&addr).copied() else {
                    out.push(ServerResponse {
                        addr,
                        packet: ServerPacket::Error(ServerError {
                            code: ServerErrorCode::NotConnected,
                            message: "send Hello first".to_string(),
                        }),
                        encoding,
                    });
                    return out;
                };

                let Some(room) = self.rooms.get_mut(&room_id) else {
                    self.addr_index.remove(&addr);
                    return out;
                };

                let Some(player) = room.players.get_mut(&player_id) else {
                    self.addr_index.remove(&addr);
                    return out;
                };

                player.last_seen = Instant::now();
                accept_input(player, input);
            }
            ClientPacket::Ping(ping) => {
                out.push(ServerResponse {
                    addr,
                    packet: ServerPacket::Pong(Pong {
                        client_time_ms: ping.client_time_ms,
                        server_time_ms: unix_time_ms(),
                    }),
                    encoding,
                });

                if let Some((room_id, player_id)) = self.addr_index.get(&addr).copied() {
                    if let Some(room) = self.rooms.get_mut(&room_id) {
                        if let Some(player) = room.players.get_mut(&player_id) {
                            player.last_seen = Instant::now();
                        }
                    }
                }
            }
            ClientPacket::Disconnect => {
                self.remove_player(addr);
            }
        }

        out
    }

    pub(crate) fn tick(&mut self, now: Instant, config: &ServerConfig, tick_dt: Duration) {
        self.tick = self.tick.wrapping_add(1);
        self.simulate(now, config, tick_dt);
    }

    pub(crate) fn cleanup_timeouts(&mut self, now: Instant, timeout: Duration) {
        let mut to_remove = Vec::new();
        for (addr, (room_id, player_id)) in self.addr_index.iter() {
            let Some(room) = self.rooms.get(room_id) else {
                continue;
            };
            let Some(player) = room.players.get(player_id) else {
                continue;
            };
            if now.duration_since(player.last_seen) > timeout {
                to_remove.push(*addr);
            }
        }
        for addr in to_remove {
            self.remove_player(addr);
        }
    }

    pub(crate) fn build_snapshot_packets(&self) -> Vec<(SocketAddr, Vec<u8>)> {
        let mut out = Vec::new();

        for (room_id, room) in self.rooms.iter() {
            let players: Vec<PlayerSnapshot> =
                room.players.iter().map(|(id, p)| p.snapshot(*id)).collect();

            let packet = ServerPacket::Snapshot(WorldSnapshot {
                tick: self.tick,
                room_id: *room_id,
                players,
            });

            let postcard_bytes = match encode_server(&packet) {
                Ok(b) => b,
                Err(err) => {
                    warn!(room_id, error = %err, "failed to encode postcard snapshot");
                    continue;
                }
            };
            let text_bytes = encode_text_server(&packet);

            for player in room.players.values() {
                let bytes = match player.encoding {
                    ClientEncoding::Postcard => postcard_bytes.clone(),
                    ClientEncoding::Text => text_bytes.clone(),
                };
                out.push((player.addr, bytes));
            }
        }

        out
    }

    pub(crate) fn summary(&self) -> DirectorySummary {
        let mut rooms: Vec<RoomSummary> = self
            .rooms
            .iter()
            .map(|(room_id, room)| RoomSummary {
                room_id: *room_id,
                player_count: room.players.len(),
            })
            .collect();
        rooms.sort_by_key(|room| room.room_id);

        DirectorySummary {
            tick: self.tick,
            room_count: rooms.len(),
            player_count: rooms.iter().map(|room| room.player_count).sum(),
            rooms,
        }
    }

    fn upsert_player(
        &mut self,
        addr: SocketAddr,
        hello: ClientHello,
        encoding: ClientEncoding,
        config: &ServerConfig,
    ) -> (RoomId, PlayerId) {
        if let Some((old_room_id, old_player_id)) = self.addr_index.remove(&addr) {
            if let Some(room) = self.rooms.get_mut(&old_room_id) {
                room.players.remove(&old_player_id);
                if room.players.is_empty() {
                    self.rooms.remove(&old_room_id);
                }
            }
        }

        let room_id = hello.room_id;
        let player_id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);
        let room = self.rooms.entry(room_id).or_default();
        let spawn = Vec3 {
            x: (player_id as f32 % 8.0) * 2.0,
            y: 0.0,
            z: ((player_id as f32 / 8.0).floor() % 8.0) * 2.0,
        };

        room.players.insert(
            player_id,
            PlayerState {
                addr,
                encoding,
                name: hello.name,
                position: spawn,
                velocity: Vec3::default(),
                yaw: 0.0,
                pitch: 0.0,
                flags: PlayerFlags::default(),
                last_processed_input: 0,
                last_input: None,
                last_seen: Instant::now(),
            },
        );
        self.addr_index.insert(addr, (room_id, player_id));

        info!(
            %addr,
            room_id,
            player_id,
            tick_rate_hz = config.tick_rate_hz,
            snapshot_rate_hz = config.snapshot_rate_hz,
            "player joined"
        );

        (room_id, player_id)
    }

    fn remove_player(&mut self, addr: SocketAddr) {
        let Some((room_id, player_id)) = self.addr_index.remove(&addr) else {
            return;
        };
        let mut name: Option<String> = None;
        if let Some(room) = self.rooms.get_mut(&room_id) {
            if let Some(player) = room.players.remove(&player_id) {
                name = player.name;
            }
            if room.players.is_empty() {
                self.rooms.remove(&room_id);
            }
        }
        info!(%addr, room_id, player_id, name = ?name, "player left");
    }

    fn simulate(&mut self, now: Instant, config: &ServerConfig, tick_dt: Duration) {
        let dt = tick_dt.as_secs_f32();

        for room in self.rooms.values_mut() {
            let room_players = room.players.len();
            for (player_id, player) in room.players.iter_mut() {
                if now.duration_since(player.last_seen) > config.player_timeout {
                    continue;
                }

                let Some(input) = player.last_input.as_ref() else {
                    player.velocity = Vec3::default();
                    continue;
                };

                let desired = simulation::movement_velocity(input, config.move_speed);
                player.velocity = desired;
                player.position = player.position.add_scaled(desired, dt);
                player.yaw = input.yaw;
                player.pitch = input.pitch;
                player.last_processed_input = player.last_processed_input.max(input.seq);

                debug!(room_players, player_id = *player_id, "sim tick");
            }
        }
    }
}

fn accept_input(player: &mut PlayerState, input: InputCommand) {
    if input.seq <= player.last_processed_input {
        return;
    }

    player.last_input = Some(simulation::sanitize_input(input));
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as u64
}
