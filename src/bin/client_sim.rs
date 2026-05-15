use std::{net::SocketAddr, time::Duration};

use sawit_service::protocol::{
    ClientHello, ClientPacket, InputCommand, MoveInput, PROTOCOL_VERSION, ServerPacket,
    decode_server, encode_client,
};
use std::sync::Arc;

use tokio::{net::UdpSocket, time};
use tracing::{debug, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    let (server_addr, room_id, name) = parse_args();

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(server_addr).await?;
    let socket = Arc::new(socket);

    info!(%server_addr, room_id, "client_sim started");

    let hello = ClientPacket::Hello(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        room_id,
        name: Some(name),
    });
    socket.send(&encode_client(&hello)?).await?;

    let (player_id, tick_hz, snapshot_hz) = wait_welcome(socket.as_ref()).await?;
    info!(player_id, tick_hz, snapshot_hz, "connected");

    let recv_socket = socket.clone();
    tokio::spawn(async move {
        if let Err(err) = recv_loop(recv_socket, player_id).await {
            warn!(error = %err, "recv loop stopped");
        }
    });

    run_input_loop(socket, tick_hz).await
}

fn parse_args() -> (SocketAddr, u64, String) {
    let mut args = std::env::args().skip(1);
    let server_addr = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:4000".to_string())
        .parse()
        .expect("server addr must be host:port");
    let room_id = args.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(1);
    let name = args.next().unwrap_or_else(|| "client_sim".to_string());
    (server_addr, room_id, name)
}

async fn wait_welcome(socket: &UdpSocket) -> anyhow::Result<(u64, u16, u16)> {
    let mut buf = vec![0u8; 16 * 1024];
    let len = time::timeout(Duration::from_secs(3), socket.recv(&mut buf)).await??;
    match decode_server(&buf[..len])? {
        ServerPacket::Welcome(w) => Ok((w.player_id, w.tick_rate_hz, w.snapshot_rate_hz)),
        other => anyhow::bail!("expected Welcome, got: {other:?}"),
    }
}

async fn recv_loop(socket: Arc<UdpSocket>, player_id: u64) -> anyhow::Result<()> {
    let mut buf = vec![0u8; 16 * 1024];
    let mut printed = 0u32;

    loop {
        let len = socket.recv(&mut buf).await?;
        let packet = match decode_server(&buf[..len]) {
            Ok(p) => p,
            Err(err) => {
                debug!(error = %err, len, "bad server packet");
                continue;
            }
        };

        match packet {
            ServerPacket::Snapshot(s) => {
                printed = printed.wrapping_add(1);
                if printed % 20 == 0 {
                    let me = s.players.iter().find(|p| p.player_id == player_id);
                    if let Some(me) = me {
                        info!(
                            tick = s.tick,
                            room_id = s.room_id,
                            players = s.players.len(),
                            me_x = me.position.x,
                            me_z = me.position.z,
                            "snapshot"
                        );
                    } else {
                        info!(
                            tick = s.tick,
                            room_id = s.room_id,
                            players = s.players.len(),
                            "snapshot"
                        );
                    }
                }
            }
            ServerPacket::Pong(p) => {
                debug!(
                    client_time_ms = p.client_time_ms,
                    server_time_ms = p.server_time_ms,
                    "pong"
                );
            }
            ServerPacket::Error(e) => {
                warn!(code = ?e.code, message = %e.message, "server error");
            }
            ServerPacket::Welcome(_) => {}
        }
    }
}

async fn run_input_loop(socket: Arc<UdpSocket>, tick_hz: u16) -> anyhow::Result<()> {
    let input_hz = 30u64;
    let mut input_seq: u32 = 0;
    let mut client_tick: u32 = 0;

    let mut input_interval = time::interval(Duration::from_millis(1000 / input_hz));
    let mut ping_interval = time::interval(Duration::from_secs(1));
    let start = time::Instant::now();

    info!(tick_hz, input_hz, "sending inputs (Ctrl+C to stop)");

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = input_interval.tick() => {
                input_seq = input_seq.wrapping_add(1);
                client_tick = client_tick.wrapping_add(1);

                let t = start.elapsed().as_secs_f32();
                let movement = MoveInput {
                    x: (t * 0.8).cos(),
                    z: (t * 0.8).sin(),
                };

                let input = ClientPacket::Input(InputCommand {
                    seq: input_seq,
                    client_tick,
                    dt_ms: (1000 / input_hz) as u16,
                    movement,
                    yaw: t,
                    pitch: 0.0,
                });

                let bytes = encode_client(&input)?;
                socket.send(&bytes).await?;
            }
            _ = ping_interval.tick() => {
                let now_ms = start.elapsed().as_millis() as u64;
                let ping = ClientPacket::Ping(sawit_service::protocol::Ping { client_time_ms: now_ms });
                let bytes = encode_client(&ping)?;
                let _ = socket.send(&bytes).await;
            }
            _ = &mut ctrl_c => {
                info!("ctrl+c");
                let _ = socket.send(&encode_client(&ClientPacket::Disconnect)?) .await;
                return Ok(());
            }
        }
    }
}
