use std::{net::SocketAddr, time::Duration};

use sawit_service::server::{self, ServerConfig};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    let mut config = ServerConfig::default();
    apply_env_overrides(&mut config);
    apply_cli_overrides(&mut config);

    info!(
        udp_bind_addr = %config.bind_addr,
        tcp_bind_addr = ?config.tcp_bind_addr,
        public_udp_addr = %config.public_udp_addr,
        "starting sawit-service"
    );
    server::run(config).await
}

fn apply_env_overrides(config: &mut ServerConfig) {
    if let Ok(bind) = std::env::var("SAWIT_BIND") {
        if let Ok(addr) = bind.parse::<SocketAddr>() {
            config.bind_addr = addr;
        }
    }
    if let Ok(bind) = std::env::var("SAWIT_TCP_BIND") {
        config.tcp_bind_addr = parse_optional_addr(&bind);
    }
    if let Ok(addr) = std::env::var("SAWIT_PUBLIC_UDP") {
        if let Ok(addr) = addr.parse::<SocketAddr>() {
            config.public_udp_addr = addr;
        }
    }
    if let Ok(v) = std::env::var("SAWIT_TICK_HZ") {
        if let Ok(hz) = v.parse::<u16>() {
            config.tick_rate_hz = hz.max(1);
        }
    }
    if let Ok(v) = std::env::var("SAWIT_SNAPSHOT_HZ") {
        if let Ok(hz) = v.parse::<u16>() {
            config.snapshot_rate_hz = hz.max(1);
        }
    }
    if let Ok(v) = std::env::var("SAWIT_MOVE_SPEED") {
        if let Ok(speed) = v.parse::<f32>() {
            config.move_speed = speed.max(0.1);
        }
    }
    if let Ok(v) = std::env::var("SAWIT_TIMEOUT_SECS") {
        if let Ok(secs) = v.parse::<u64>() {
            config.player_timeout = Duration::from_secs(secs.max(1));
        }
    }
}

fn apply_cli_overrides(config: &mut ServerConfig) {
    // MVP CLI: `cargo run --bin sawit-service -- 0.0.0.0:4000`
    // Use env vars for TCP/public address and other knobs.
    let mut args = std::env::args().skip(1);
    if let Some(first) = args.next() {
        if let Ok(addr) = first.parse::<SocketAddr>() {
            config.bind_addr = addr;
        }
    }
}

fn parse_optional_addr(value: &str) -> Option<SocketAddr> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("off")
        || value.eq_ignore_ascii_case("none")
        || value.eq_ignore_ascii_case("disabled")
        || value == "0"
    {
        return None;
    }

    value.parse::<SocketAddr>().ok()
}
