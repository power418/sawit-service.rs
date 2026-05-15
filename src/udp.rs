use std::{sync::Arc, time::Duration};

use tokio::{net::UdpSocket, sync::Mutex, time};
use tracing::{debug, info, warn};

use crate::{
    room::RoomDirectory,
    server::ServerConfig,
    wire::{ServerResponse, decode_received_client, encode_for_client},
};

pub(crate) async fn run(
    config: ServerConfig,
    rooms: Arc<Mutex<RoomDirectory>>,
) -> anyhow::Result<()> {
    let socket = UdpSocket::bind(config.bind_addr).await?;
    let socket = Arc::new(socket);

    info!(
        bind_addr = %config.bind_addr,
        tick_rate_hz = config.tick_rate_hz,
        snapshot_rate_hz = config.snapshot_rate_hz,
        "UDP realtime gateway started"
    );

    let recv_socket = socket.clone();
    let recv_rooms = rooms.clone();
    let recv_config = config.clone();
    tokio::spawn(async move {
        if let Err(err) = recv_loop(recv_socket, recv_rooms, recv_config).await {
            warn!(error = %err, "UDP recv loop stopped");
        }
    });

    tick_loop(socket, rooms, config).await
}

async fn recv_loop(
    socket: Arc<UdpSocket>,
    rooms: Arc<Mutex<RoomDirectory>>,
    config: ServerConfig,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; config.max_packet_bytes];

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;
        if len == 0 {
            continue;
        }

        let packet = match decode_received_client(&buf[..len]) {
            Ok(p) => p,
            Err(err) => {
                debug!(%addr, error = %err, len, "malformed client packet");
                continue;
            }
        };

        let responses = {
            let mut rooms = rooms.lock().await;
            rooms.handle_client_packet(addr, packet, &config)
        };
        send_responses(socket.as_ref(), responses).await;
    }
}

async fn tick_loop(
    socket: Arc<UdpSocket>,
    rooms: Arc<Mutex<RoomDirectory>>,
    config: ServerConfig,
) -> anyhow::Result<()> {
    let tick_dt = Duration::from_secs_f32(1.0 / config.tick_rate_hz.max(1) as f32);
    let snapshot_dt = Duration::from_secs_f32(1.0 / config.snapshot_rate_hz.max(1) as f32);

    let mut tick_interval = time::interval(tick_dt);
    let mut snapshot_interval = time::interval(snapshot_dt);

    loop {
        tokio::select! {
            _ = tick_interval.tick() => {
                let mut rooms = rooms.lock().await;
                rooms.tick(std::time::Instant::now(), &config, tick_dt);
            }
            _ = snapshot_interval.tick() => {
                let packets = {
                    let mut rooms = rooms.lock().await;
                    rooms.cleanup_timeouts(std::time::Instant::now(), config.player_timeout);
                    rooms.build_snapshot_packets()
                };
                send_snapshot_packets(socket.as_ref(), packets).await;
            }
        }
    }
}

async fn send_snapshot_packets(socket: &UdpSocket, packets: Vec<(std::net::SocketAddr, Vec<u8>)>) {
    for (addr, bytes) in packets {
        if let Err(err) = socket.send_to(&bytes, addr).await {
            debug!(%addr, error = %err, "send snapshot failed");
        }
    }
}

async fn send_responses(socket: &UdpSocket, responses: Vec<ServerResponse>) {
    for response in responses {
        let bytes = match encode_for_client(&response.packet, response.encoding) {
            Ok(b) => b,
            Err(err) => {
                debug!(addr = %response.addr, error = %err, "encode server packet failed");
                continue;
            }
        };
        if let Err(err) = socket.send_to(&bytes, response.addr).await {
            debug!(addr = %response.addr, error = %err, "send response failed");
        }
    }
}
