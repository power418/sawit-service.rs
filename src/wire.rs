use std::{fmt::Write as _, net::SocketAddr};

use crate::protocol::{
    ClientHello, ClientPacket, InputCommand, MoveInput, ServerPacket, decode_client, encode_server,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientEncoding {
    Postcard,
    Text,
}

#[derive(Debug)]
pub(crate) struct ReceivedClientPacket {
    pub packet: ClientPacket,
    pub encoding: ClientEncoding,
}

#[derive(Debug)]
pub(crate) struct ServerResponse {
    pub addr: SocketAddr,
    pub packet: ServerPacket,
    pub encoding: ClientEncoding,
}

pub(crate) fn decode_received_client(bytes: &[u8]) -> anyhow::Result<ReceivedClientPacket> {
    match decode_client(bytes) {
        Ok(packet) => Ok(ReceivedClientPacket {
            packet,
            encoding: ClientEncoding::Postcard,
        }),
        Err(postcard_err) => match decode_text_client(bytes) {
            Ok(packet) => Ok(ReceivedClientPacket {
                packet,
                encoding: ClientEncoding::Text,
            }),
            Err(text_err) => anyhow::bail!("postcard={postcard_err}; text={text_err}"),
        },
    }
}

pub(crate) fn encode_for_client(
    packet: &ServerPacket,
    encoding: ClientEncoding,
) -> Result<Vec<u8>, postcard::Error> {
    match encoding {
        ClientEncoding::Postcard => encode_server(packet),
        ClientEncoding::Text => Ok(encode_text_server(packet)),
    }
}

pub(crate) fn encode_text_server(packet: &ServerPacket) -> Vec<u8> {
    let mut text = String::new();
    match packet {
        ServerPacket::Welcome(w) => {
            let _ = writeln!(
                text,
                "WELCOME {} {} {} {} {}",
                w.protocol_version, w.player_id, w.room_id, w.tick_rate_hz, w.snapshot_rate_hz
            );
        }
        ServerPacket::Snapshot(snapshot) => {
            let _ = write!(
                text,
                "SNAPSHOT {} {} {}",
                snapshot.tick,
                snapshot.room_id,
                snapshot.players.len()
            );
            for player in snapshot.players.iter() {
                let grounded = if player.flags.grounded { 1 } else { 0 };
                let _ = write!(
                    text,
                    " {} {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} {:.5} {:.5} {} {}",
                    player.player_id,
                    player.position.x,
                    player.position.y,
                    player.position.z,
                    player.velocity.x,
                    player.velocity.y,
                    player.velocity.z,
                    player.yaw,
                    player.pitch,
                    grounded,
                    player.last_processed_input
                );
            }
            text.push('\n');
        }
        ServerPacket::Pong(p) => {
            let _ = writeln!(text, "PONG {} {}", p.client_time_ms, p.server_time_ms);
        }
        ServerPacket::Error(e) => {
            let message = e.message.replace(['\r', '\n'], " ");
            let _ = writeln!(text, "ERROR {:?} {}", e.code, message);
        }
    }
    text.into_bytes()
}

fn decode_text_client(bytes: &[u8]) -> anyhow::Result<ClientPacket> {
    let text = std::str::from_utf8(bytes)?.trim_matches(|c| c == '\0' || c == '\r' || c == '\n');
    let mut parts = text.split_whitespace();
    let Some(kind) = parts.next() else {
        anyhow::bail!("empty text packet");
    };

    match kind {
        "HELLO" => {
            let protocol_version = parse_next(&mut parts, "protocol_version")?;
            let room_id = parse_next(&mut parts, "room_id")?;
            let name = parts
                .next()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            Ok(ClientPacket::Hello(ClientHello {
                protocol_version,
                room_id,
                name,
            }))
        }
        "INPUT" => Ok(ClientPacket::Input(InputCommand {
            seq: parse_next(&mut parts, "seq")?,
            client_tick: parse_next(&mut parts, "client_tick")?,
            dt_ms: parse_next(&mut parts, "dt_ms")?,
            movement: MoveInput {
                x: parse_next(&mut parts, "move_x")?,
                z: parse_next(&mut parts, "move_z")?,
            },
            yaw: parse_next(&mut parts, "yaw")?,
            pitch: parse_next(&mut parts, "pitch")?,
        })),
        "PING" => Ok(ClientPacket::Ping(crate::protocol::Ping {
            client_time_ms: parse_next(&mut parts, "client_time_ms")?,
        })),
        "DISCONNECT" => Ok(ClientPacket::Disconnect),
        _ => anyhow::bail!("unknown text packet kind={kind}"),
    }
}

fn parse_next<T: std::str::FromStr>(
    parts: &mut std::str::SplitWhitespace<'_>,
    label: &'static str,
) -> anyhow::Result<T>
where
    T::Err: std::fmt::Display,
{
    let value = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing {label}"))?;
    value
        .parse::<T>()
        .map_err(|err| anyhow::anyhow!("bad {label}={value}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{PROTOCOL_VERSION, RoomId};

    #[test]
    fn decodes_text_hello() {
        let packet = decode_received_client(b"HELLO 1 42 alice\n").unwrap();
        assert_eq!(packet.encoding, ClientEncoding::Text);
        let ClientPacket::Hello(hello) = packet.packet else {
            panic!("expected hello");
        };
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
        assert_eq!(hello.room_id, 42 as RoomId);
        assert_eq!(hello.name.as_deref(), Some("alice"));
    }
}
