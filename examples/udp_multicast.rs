#![forbid(unsafe_code)]

use async_trait::async_trait;
use maestro_rs::{NetworkInterface, Result, Supervisor, UdpHandler};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
    sync::Arc,
};
use tokio::net::UdpSocket;
use tracing::{error, info};

struct MulticastUdp;

#[async_trait]
impl UdpHandler for MulticastUdp {
    fn name(&self) -> &'static str {
        "Multicast UDP Example"
    }

    fn port(&self) -> u16 {
        5353
    }

    fn multicast_addrs(&self) -> &[IpAddr] {
        static GROUPS: [IpAddr; 1] = [IpAddr::V4(Ipv4Addr::new(239, 255, 0, 1))];
        &GROUPS
    }

    async fn on_packet(&self, data: &[u8], socket: Arc<UdpSocket>, peer: &SocketAddr) {
        info!("Received multicast packet from {}: {:?}", peer, data);
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

    supervisor.add(MulticastUdp);
    supervisor.run().await?;

    Ok(())
}
