//! UDP service that joins the multicast group 239.255.0.1 on port 5354.
//!
//! ```sh
//! cargo run --example udp_multicast [INTERFACE]   # default: the loopback interface
//! # from another shell (Linux):
//! echo hi | socat - UDP4-DATAGRAM:239.255.0.1:5354,ip-multicast-if=127.0.0.1
//! ```

use maestro_rs::{NetworkInterface, Result, Supervisor, UdpHandler};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::net::UdpSocket;
use tracing::{info, warn};

const GROUPS: [IpAddr; 1] = [IpAddr::V4(Ipv4Addr::new(239, 255, 0, 1))];

struct MulticastUdp;

impl UdpHandler for MulticastUdp {
    fn name(&self) -> &str {
        "udp-multicast"
    }

    fn port(&self) -> u16 {
        5354
    }

    fn multicast_groups(&self) -> &[IpAddr] {
        &GROUPS
    }

    async fn on_packet(&self, data: &[u8], socket: &Arc<UdpSocket>, peer: SocketAddr) {
        info!(%peer, len = data.len(), "datagram received");
        if let Err(e) = socket.send_to(b"ACK", peer).await {
            warn!(%peer, error = %e, "reply failed");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let iface = match std::env::args().nth(1) {
        Some(name) => name.parse()?,
        None => NetworkInterface::loopback()?,
    };
    let mut supervisor = Supervisor::new(iface);
    supervisor.add(MulticastUdp);
    supervisor.run().await
}
