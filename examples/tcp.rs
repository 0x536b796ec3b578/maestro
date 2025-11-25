#![forbid(unsafe_code)]

use maestro_rs::{NetworkInterface, Result, Supervisor, TcpHandler, async_trait};
use std::{net::SocketAddr, str::FromStr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::{error, info};

struct EchoTcp;

#[async_trait]
impl TcpHandler for EchoTcp {
    fn name(&self) -> &'static str {
        "TCP Echo Service"
    }

    fn port(&self) -> u16 {
        8080
    }

    async fn on_connection(&self, mut stream: TcpStream, peer: &SocketAddr) {
        info!("New TCP client: {}", peer);

        let mut buf = [0u8; 1024];

        loop {
            match stream.read(&mut buf).await {
                Ok(0) => {
                    info!("Client {} disconnected", peer);
                    return;
                }
                Ok(n) => {
                    if let Err(e) = stream.write_all(&buf[..n]).await {
                        error!("TCP write failed: {:?}", e);
                        return;
                    }
                }
                Err(e) => {
                    error!("TCP read failed from {}: {:?}", peer, e);
                    return;
                }
            }
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

    supervisor.add(EchoTcp);
    supervisor.run().await?;

    Ok(())
}
