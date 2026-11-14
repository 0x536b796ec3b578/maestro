//! UDP echo server.
//!
//! ```sh
//! cargo run --example udp [INTERFACE]   # default: the loopback interface
//! nc -u 127.0.0.1 5353
//! ```

use maestro_rs::{NetworkInterface, Result, Supervisor, UdpHandler};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::UdpSocket;
use tracing::{info, warn};

struct EchoUdp;

impl UdpHandler for EchoUdp {
    fn name(&self) -> &str {
        "udp-echo"
    }

    fn port(&self) -> u16 {
        5353
    }

    async fn on_packet(&self, data: &[u8], socket: &Arc<UdpSocket>, peer: SocketAddr) {
        info!(%peer, len = data.len(), "datagram received");
        if let Err(e) = socket.send_to(data, peer).await {
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
    supervisor.add(EchoUdp);
    supervisor.run().await
}
