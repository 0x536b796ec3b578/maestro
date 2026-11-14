//! Manual Load Tester for UDP
//!
//! Run with:
//! cargo bench --bench udp_bench -- --mode server
//! cargo bench --bench udp_bench -- --mode client

use clap::Parser;
use maestro_rs::{NetworkInterface, Result, Supervisor, UdpHandler, async_trait};
use std::{
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::net::UdpSocket;

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
impl UdpHandler for BenchmarkServer {
    fn name(&self) -> &'static str {
        "BenchmarkServer"
    }
    fn port(&self) -> u16 {
        9999
    }

    async fn on_packet(&self, _data: &[u8], _sock: Arc<UdpSocket>, _peer: &SocketAddr) {
        self.counter.fetch_add(1, Ordering::Relaxed);
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
                let pps = current - last_val;
                println!("Packets per second: {}", pps);
                last_val = current;
            }
        });

        let iface = NetworkInterface::from_str("lo")?;
        let mut supervisor = Supervisor::new(iface);
        supervisor.add(BenchmarkServer { counter });
        supervisor.run().await?;
    } else {
        // Client Flooder
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let target: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        println!("Flooding {} with UDP connections...", target);

        let payload = vec![0u8; 64];
        let start = Instant::now();
        let mut count = 0;

        while start.elapsed().as_secs() < 10 {
            // Send a batch to avoid checking time too often
            for _ in 0..100 {
                socket.send_to(&payload, target).await?;
                count += 1;
            }
        }

        println!("Total packets sent: {}", count);
        println!("Average rate: {} pp/s", count / 10);
    }
    Ok(())
}
