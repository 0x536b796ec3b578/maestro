//! Manual TCP load test: connections accepted per second.
//!
//! ```sh
//! cargo run --release --example load_tcp -- server
//! cargo run --release --example load_tcp -- client   # in another shell
//! ```

use clap::{Parser, ValueEnum};
use maestro_rs::{NetworkInterface, Result, Supervisor, TcpHandler};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpStream, time::Instant};

const PORT: u16 = 9998;
const CLIENT_TASKS: usize = 16;
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

impl TcpHandler for Counter {
    fn name(&self) -> &str {
        "load-tcp"
    }
    fn port(&self) -> u16 {
        PORT
    }

    async fn on_connection(&self, _stream: TcpStream, _peer: SocketAddr) {
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
            println!("connections/s: {}", now - last);
            last = now;
        }
    });

    let mut supervisor = Supervisor::new(NetworkInterface::loopback()?);
    supervisor.add(Counter(count));
    supervisor.run().await
}

async fn client() {
    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
    println!("connecting to {target} for {DURATION:?} with {CLIENT_TASKS} tasks");
    let deadline = Instant::now() + DURATION;

    let tasks: Vec<_> = (0..CLIENT_TASKS)
        .map(|_| {
            tokio::spawn(async move {
                let mut ok = 0u64;
                while Instant::now() < deadline {
                    if TcpStream::connect(target).await.is_ok() {
                        ok += 1;
                    }
                }
                ok
            })
        })
        .collect();

    let mut total = 0;
    for task in tasks {
        total += task.await.unwrap_or(0);
    }
    println!("connections: {total} ({} /s)", total / DURATION.as_secs());
}

#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().mode {
        Mode::Server => server().await,
        Mode::Client => {
            client().await;
            Ok(())
        }
    }
}
