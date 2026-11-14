//! Choosing the interface from the command line with clap, plus a custom
//! restart policy and both protocols on one supervisor.
//!
//! ```sh
//! cargo run --example clap_iface -- --interface lo
//! cargo run --example clap_iface -- --list
//! ```

use clap::{Parser, value_parser};
use maestro_rs::{NetworkInterface, RestartPolicy, Result, Supervisor, TcpHandler, UdpHandler};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
};
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    /// Network interface to bind services on.
    #[arg(
        short,
        long,
        value_name = "IFACE",
        value_parser = value_parser!(NetworkInterface),
        required_unless_present = "list"
    )]
    interface: Option<NetworkInterface>,

    /// List available interfaces and exit.
    #[arg(long)]
    list: bool,
}

struct EchoTcp;

impl TcpHandler for EchoTcp {
    fn name(&self) -> &str {
        "cli-tcp"
    }
    fn port(&self) -> u16 {
        8080
    }

    async fn on_connection(&self, mut stream: TcpStream, peer: SocketAddr) {
        info!(%peer, "TCP client connected");
        let mut buf = [0u8; 1024];
        while let Ok(n @ 1..) = stream.read(&mut buf).await {
            if stream.write_all(&buf[..n]).await.is_err() {
                break;
            }
        }
    }
}

struct AckUdp;

impl UdpHandler for AckUdp {
    fn name(&self) -> &str {
        "cli-udp"
    }
    fn port(&self) -> u16 {
        5353
    }

    async fn on_packet(&self, data: &[u8], socket: &Arc<UdpSocket>, peer: SocketAddr) {
        info!(%peer, len = data.len(), "UDP datagram");
        let _ = socket.send_to(b"ACK", peer).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("debug").init();
    let args = Args::parse();

    if args.list {
        for iface in NetworkInterface::list()? {
            let alias = iface.alias().map(|a| format!(" ({a})")).unwrap_or_default();
            println!("{iface}{alias}: {:?} {:?}", iface.ipv4(), iface.ipv6());
        }
        return Ok(());
    }

    let iface = args
        .interface
        .expect("clap enforces --interface unless --list");
    let mut supervisor = Supervisor::new(iface)
        .with_restart_policy(RestartPolicy::default().with_unlimited_restarts())
        .with_shutdown_timeout(Duration::from_secs(2));
    supervisor.add(EchoTcp).add(AckUdp);
    supervisor.run().await
}
