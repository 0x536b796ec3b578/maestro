#![forbid(unsafe_code)]
//! Run with:
//! cargo run --example udp

use maestro_rs::{NetworkInterface, Result, Supervisor, UdpHandler, async_trait};
use std::{net::SocketAddr, str::FromStr, sync::Arc};
use tokio::net::UdpSocket;
use tracing::{error, info};

struct EchoUdp;

#[async_trait]
impl UdpHandler for EchoUdp {
    fn name(&self) -> &'static str {
        "UDP Echo Service"
    }

    fn port(&self) -> u16 {
        5353
    }

    async fn on_packet(&self, data: &[u8], socket: Arc<UdpSocket>, peer: &SocketAddr) {
        info!("UDP packet from {}: {:?}", peer, data);

        if let Err(e) = socket.send_to(b"ACK", peer).await {
            error!("Failed to send UDP response: {:?}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let iface = NetworkInterface::from_str("lo")?;
    let mut supervisor = Supervisor::new(iface);

    supervisor.add(EchoUdp);
    supervisor.run().await?;

    Ok(())
}
