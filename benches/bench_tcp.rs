//! Manual Load Tester for TCP
//!
//! Run with:
//! cargo bench --bench tcp_bench -- --mode server
//! cargo bench --bench tcp_bench -- --mode client

use async_trait::async_trait;
use clap::Parser;
use maestro_rs::{NetworkInterface, Result, Supervisor, TcpHandler};
use std::{
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, net::TcpStream};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    mode: String, // "server" or "client"

    // Cargo automatically passes --bench, so we must define it to avoid parsing errors.
    #[arg(long, hide = true)]
    bench: bool,
}

struct BenchmarkServer {
    counter: Arc<AtomicUsize>,
}

#[async_trait]
impl TcpHandler for BenchmarkServer {
    fn name(&self) -> &'static str {
        "BenchmarkServer"
    }
    fn port(&self) -> u16 {
        9998
    }

    async fn on_connection(&self, mut stream: TcpStream, _peer: &SocketAddr) {
        self.counter.fetch_add(1, Ordering::Relaxed);
        let mut buf = [0u8; 1];
        let _ = stream.read_exact(&mut buf).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.mode == "server" {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // Stats Printer
        tokio::spawn(async move {
            let mut last_val = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let current = counter_clone.load(Ordering::Relaxed);
                let cps = current - last_val;
                println!("Connections per second: {}", cps);
                last_val = current;
            }
        });

        let iface = NetworkInterface::from_str("lo")?;
        let mut supervisor = Supervisor::new(iface);
        supervisor.add(BenchmarkServer { counter });
        supervisor.run().await?;
    } else {
        // Client Flooder
        let target: SocketAddr = "127.0.0.1:9998".parse().unwrap();
        println!("Flooding {} with TCP connections...", target);

        // Spawn multiple connector tasks
        let mut handles = vec![];
        for _ in 0..16 {
            handles.push(tokio::spawn(async move {
                let mut count = 0;
                let end_time = Instant::now() + Duration::from_secs(10);

                while Instant::now() < end_time {
                    if let Ok(stream) = TcpStream::connect(target).await {
                        // Close immediately to measure handshake performance
                        let _ = stream.set_nodelay(true);
                    }
                    count += 1;
                }
                count
            }));
        }

        let mut total = 0;
        for h in handles {
            total += h.await.unwrap();
        }

        println!("Total connections initiated: {}", total);
        println!("Average rate: {} cp/s", total / 10);
    }
    Ok(())
}
