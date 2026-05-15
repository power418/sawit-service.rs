use std::{net::SocketAddr, sync::Arc};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tracing::{debug, info, warn};

use crate::{protocol::RoomId, room::RoomDirectory, server::ServerConfig};

pub(crate) async fn run(
    bind_addr: SocketAddr,
    config: ServerConfig,
    rooms: Arc<Mutex<RoomDirectory>>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    info!(
        bind_addr = %bind_addr,
        udp_addr = %config.public_udp_addr,
        "TCP control gateway started"
    );

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let config = config.clone();
        let rooms = rooms.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_control_connection(stream, peer_addr, config, rooms).await {
                debug!(%peer_addr, error = %err, "TCP control connection closed");
            }
        });
    }
}

async fn handle_control_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    config: ServerConfig,
    rooms: Arc<Mutex<RoomDirectory>>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    writer
        .write_all(b"SAWIT_CONTROL 1 commands=JOIN,ROOMS,HEALTH,HELP,QUIT\n")
        .await?;

    loop {
        line.clear();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Ok(());
        }
        if line.len() > 1024 {
            warn!(%peer_addr, "TCP control line too large");
            writer.write_all(b"ERR line_too_large\n").await?;
            continue;
        }

        let response = handle_command(line.trim(), &config, rooms.clone()).await;
        let should_quit = response == "BYE\n";
        writer.write_all(response.as_bytes()).await?;
        if should_quit {
            return Ok(());
        }
    }
}

async fn handle_command(
    command: &str,
    config: &ServerConfig,
    rooms: Arc<Mutex<RoomDirectory>>,
) -> String {
    let mut parts = command.split_whitespace();
    let Some(kind) = parts.next() else {
        return "ERR empty_command\n".to_string();
    };

    match kind {
        "JOIN" => {
            let room_id = parts
                .next()
                .and_then(|value| value.parse::<RoomId>().ok())
                .unwrap_or(1);
            let _name = parts.next().unwrap_or("player");
            format!(
                "JOIN_OK room_id={} udp_addr={} join_token=dev-local tick_rate_hz={} snapshot_rate_hz={}\n",
                room_id, config.public_udp_addr, config.tick_rate_hz, config.snapshot_rate_hz
            )
        }
        "ROOMS" => {
            let summary = rooms.lock().await.summary();
            let mut response = format!(
                "ROOMS tick={} room_count={} player_count={}",
                summary.tick, summary.room_count, summary.player_count
            );
            for room in summary.rooms {
                response.push_str(&format!(" {}:{}", room.room_id, room.player_count));
            }
            response.push('\n');
            response
        }
        "HEALTH" => {
            let summary = rooms.lock().await.summary();
            format!(
                "OK udp_addr={} rooms={} players={} tick={} tick_rate_hz={} snapshot_rate_hz={}\n",
                config.public_udp_addr,
                summary.room_count,
                summary.player_count,
                summary.tick,
                config.tick_rate_hz,
                config.snapshot_rate_hz
            )
        }
        "HELP" => "HELP JOIN <room_id> <name> | ROOMS | HEALTH | QUIT\n".to_string(),
        "QUIT" => "BYE\n".to_string(),
        _ => "ERR unknown_command\n".to_string(),
    }
}
