#![forbid(unsafe_code)]

use clap::{Parser, value_parser};
use maestro_rs::{NetworkInterface, Result, Supervisor, TcpHandler, UdpHandler, async_trait};
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
};
use tracing::{error, info};

#[derive(Parser, Debug)]
pub struct Parameters {
    /// Network interface to bind services on.
    #[arg(
        short = 'i',
        long = "interface",
        value_name = "IFACE",
        value_parser = value_parser!(NetworkInterface)
    )]
    pub network_interface: NetworkInterface,
}

struct MyTcp;

#[async_trait]
impl TcpHandler for MyTcp {
    fn name(&self) -> &'static str {
        "CLI TCP Example"
    }
    fn port(&self) -> u16 {
        8080
    }

    async fn on_connection(&self, mut stream: TcpStream, peer: &SocketAddr) {
        info!("TCP client connected: {}", peer);
        let mut buf = [0u8; 1024];

        loop {
            let n = match stream.read(&mut buf).await {
                Ok(0) => return,
                Ok(n) => n,
                Err(e) => {
                    error!("TCP read failure: {:?}", e);
                    return;
                }
            };

            if let Err(e) = stream.write_all(&buf[..n]).await {
                error!("TCP write failure: {:?}", e);
                return;
            }
        }
    }
}

struct MyUdp;

#[async_trait]
impl UdpHandler for MyUdp {
    fn name(&self) -> &'static str {
        "CLI UDP Example"
    }
    fn port(&self) -> u16 {
        5353
    }

    async fn on_packet(&self, data: &[u8], socket: Arc<UdpSocket>, peer: &SocketAddr) {
        info!("UDP packet from {}: {:?}", peer, data);
        let _ = socket.send_to(b"ACK", peer).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let params = Parameters::parse();
    let mut supervisor = Supervisor::new(params.network_interface);

    supervisor.add(MyTcp);
    supervisor.add(MyUdp);

    supervisor.run().await?;

    Ok(())
}
