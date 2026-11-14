//! TCP echo server.
//!
//! ```sh
//! cargo run --example tcp [INTERFACE]   # default: the loopback interface
//! nc 127.0.0.1 8080
//! ```

use maestro_rs::{NetworkInterface, Result, Supervisor, TcpHandler};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::{info, warn};

struct EchoTcp;

impl TcpHandler for EchoTcp {
    fn name(&self) -> &str {
        "tcp-echo"
    }

    fn port(&self) -> u16 {
        8080
    }

    async fn on_connection(&self, mut stream: TcpStream, peer: SocketAddr) {
        info!(%peer, "client connected");
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = stream.write_all(&buf[..n]).await {
                        warn!(%peer, error = %e, "write failed");
                        break;
                    }
                }
                Err(e) => {
                    warn!(%peer, error = %e, "read failed");
                    break;
                }
            }
        }
        info!(%peer, "client disconnected");
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
    supervisor.add(EchoTcp);
    supervisor.run().await
}
