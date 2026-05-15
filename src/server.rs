use std::{net::SocketAddr, sync::Arc, time::Duration};

use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::{protocol::MAX_PACKET_BYTES, room::RoomDirectory};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub public_udp_addr: SocketAddr,
    pub tcp_bind_addr: Option<SocketAddr>,
    pub tick_rate_hz: u16,
    pub snapshot_rate_hz: u16,
    pub player_timeout: Duration,
    pub max_packet_bytes: usize,
    pub move_speed: f32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:4000".parse().expect("valid default UDP bind"),
            public_udp_addr: "127.0.0.1:4000"
                .parse()
                .expect("valid default public UDP addr"),
            tcp_bind_addr: Some("0.0.0.0:4001".parse().expect("valid default TCP bind")),
            tick_rate_hz: 20,
            snapshot_rate_hz: 20,
            player_timeout: Duration::from_secs(10),
            max_packet_bytes: MAX_PACKET_BYTES,
            move_speed: 6.2,
        }
    }
}

pub async fn run(config: ServerConfig) -> anyhow::Result<()> {
    let rooms = Arc::new(Mutex::new(RoomDirectory::default()));

    info!(
        udp_bind_addr = %config.bind_addr,
        public_udp_addr = %config.public_udp_addr,
        tcp_bind_addr = ?config.tcp_bind_addr,
        tick_rate_hz = config.tick_rate_hz,
        snapshot_rate_hz = config.snapshot_rate_hz,
        "starting sawit-service game server"
    );

    if let Some(tcp_bind_addr) = config.tcp_bind_addr {
        let tcp_config = config.clone();
        let tcp_rooms = rooms.clone();
        tokio::spawn(async move {
            if let Err(err) = crate::tcp::run(tcp_bind_addr, tcp_config, tcp_rooms).await {
                warn!(error = %err, bind_addr = %tcp_bind_addr, "TCP control gateway stopped");
            }
        });
    } else {
        info!("TCP control gateway disabled");
    }

    crate::udp::run(config, rooms).await
}
