//! Manual UDP load test: datagrams handled per second.
//!
//! ```sh
//! cargo run --release --example load_udp -- server
//! cargo run --release --example load_udp -- client   # in another shell
//! ```

use clap::{Parser, ValueEnum};
use maestro_rs::{NetworkInterface, Result, Supervisor, UdpHandler};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{net::UdpSocket, time::Instant};

const PORT: u16 = 9999;
const PAYLOAD: [u8; 64] = [0; 64];
const DURATION: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Server,
    Client,
}

#[derive(Parser)]
struct Args {
    #[arg(value_enum)]
    mode: Mode,
}

struct Counter(Arc<AtomicU64>);

impl UdpHandler for Counter {
    fn name(&self) -> &str {
        "load-udp"
    }
    fn port(&self) -> u16 {
        PORT
    }

    async fn on_packet(&self, _data: &[u8], _socket: &Arc<UdpSocket>, _peer: SocketAddr) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

async fn server() -> Result<()> {
    let count = Arc::new(AtomicU64::new(0));
    let reporter = count.clone();
    tokio::spawn(async move {
        let mut last = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let now = reporter.load(Ordering::Relaxed);
            println!("datagrams/s: {}", now - last);
            last = now;
        }
    });

    let mut supervisor = Supervisor::new(NetworkInterface::loopback()?);
    supervisor.add(Counter(count));
    supervisor.run().await
}

async fn client() -> Result<()> {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
    println!("sending to {target} for {DURATION:?}");

    let deadline = Instant::now() + DURATION;
    let mut sent = 0u64;
    while Instant::now() < deadline {
        // Check the clock once per batch, not per datagram.
        for _ in 0..100 {
            socket.send_to(&PAYLOAD, target).await?;
        }
        sent += 100;
    }
    println!("datagrams sent: {sent} ({} /s)", sent / DURATION.as_secs());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().mode {
        Mode::Server => server().await,
        Mode::Client => client().await,
    }
}
