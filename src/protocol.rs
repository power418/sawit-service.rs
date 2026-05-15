use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

// UDP payload hard-limit for this MVP implementation.
// Keep this small-ish to reduce fragmentation; tune later with delta compression.
pub const MAX_PACKET_BYTES: usize = 16 * 1024;

pub type PlayerId = u64;
pub type RoomId = u64;
pub type Tick = u32;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn add_scaled(self, v: Vec3, s: f32) -> Vec3 {
        Vec3 {
            x: self.x + v.x * s,
            y: self.y + v.y * s,
            z: self.z + v.z * s,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct MoveInput {
    /// Left(-) / Right(+)
    pub x: f32,
    /// Back(-) / Forward(+)
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientHello {
    pub protocol_version: u16,
    pub room_id: RoomId,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Welcome {
    pub protocol_version: u16,
    pub player_id: PlayerId,
    pub room_id: RoomId,
    pub tick_rate_hz: u16,
    pub snapshot_rate_hz: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Ping {
    pub client_time_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Pong {
    pub client_time_ms: u64,
    pub server_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputCommand {
    pub seq: u32,
    pub client_tick: Tick,
    pub dt_ms: u16,
    pub movement: MoveInput,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PlayerFlags {
    pub grounded: bool,
}

impl Default for PlayerFlags {
    fn default() -> Self {
        Self { grounded: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub player_id: PlayerId,
    pub position: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: PlayerFlags,
    pub last_processed_input: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub tick: Tick,
    pub room_id: RoomId,
    pub players: Vec<PlayerSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerError {
    pub code: ServerErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ServerErrorCode {
    BadProtocolVersion,
    NotConnected,
    MalformedPacket,
    RateLimited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientPacket {
    Hello(ClientHello),
    Input(InputCommand),
    Ping(Ping),
    Disconnect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerPacket {
    Welcome(Welcome),
    Snapshot(WorldSnapshot),
    Pong(Pong),
    Error(ServerError),
}

pub fn encode_server(packet: &ServerPacket) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_stdvec(packet)
}

pub fn encode_client(packet: &ClientPacket) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_stdvec(packet)
}

pub fn decode_client(bytes: &[u8]) -> Result<ClientPacket, postcard::Error> {
    postcard::from_bytes(bytes)
}

pub fn decode_server(bytes: &[u8]) -> Result<ServerPacket, postcard::Error> {
    postcard::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_hello() {
        let pkt = ClientPacket::Hello(ClientHello {
            protocol_version: PROTOCOL_VERSION,
            room_id: 1,
            name: Some("alice".to_string()),
        });
        let bytes = encode_client(&pkt).unwrap();
        let decoded = decode_client(&bytes).unwrap();
        assert!(matches!(decoded, ClientPacket::Hello(_)));
    }

    #[test]
    fn roundtrip_snapshot() {
        let pkt = ServerPacket::Snapshot(WorldSnapshot {
            tick: 123,
            room_id: 1,
            players: vec![PlayerSnapshot {
                player_id: 42,
                position: Vec3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                velocity: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                yaw: 0.1,
                pitch: 0.2,
                flags: PlayerFlags { grounded: true },
                last_processed_input: 9,
            }],
        });
        let bytes = encode_server(&pkt).unwrap();
        let decoded = decode_server(&bytes).unwrap();
        assert!(matches!(decoded, ServerPacket::Snapshot(_)));
    }
}
