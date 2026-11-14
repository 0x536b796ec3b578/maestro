//! UDP benchmarks: latency and ingest throughput through a real Maestro server on loopback.
//!
//! ```sh
//! cargo bench --bench udp
//! cargo bench --bench udp -- ingest
//! ```
//!
//! `udp/ingest/N` sends from N client sockets at once. On Linux each client
//! socket is hashed to one of Maestro's `SO_REUSEPORT` sockets, so throughput
//! should grow with N up to the core count. Elsewhere all clients share one
//! socket read by one receive loop per core.

mod common;

use common::{LOCALHOST, Server, client_runtime, cores, free_port, wait_until};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use maestro_rs::{BindMode, UdpHandler};
use std::{
    hint::black_box,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{net::UdpSocket, sync::Notify, task::JoinSet, time::Instant};

/// Datagrams each client sends per iteration. Small enough to never overflow
/// the socket buffers on loopback, so nothing is dropped.
const BATCH: u64 = 64;
const PAYLOAD: [u8; 64] = [7; 64];

struct Echo {
    port: u16,
}

impl UdpHandler for Echo {
    fn name(&self) -> &str {
        "bench-udp-echo"
    }
    fn port(&self) -> u16 {
        self.port
    }
    fn bind_mode(&self) -> BindMode {
        BindMode::Specific(LOCALHOST)
    }
    async fn on_packet(&self, data: &[u8], socket: &Arc<UdpSocket>, peer: SocketAddr) {
        let _ = socket.send_to(data, peer).await;
    }
}

/// Counts datagrams and wakes the client once `target` is reached.
#[derive(Default)]
struct Progress {
    received: AtomicU64,
    target: AtomicU64,
    reached: Notify,
}

struct Sink {
    port: u16,
    progress: Arc<Progress>,
}

impl UdpHandler for Sink {
    fn name(&self) -> &str {
        "bench-udp-sink"
    }
    fn port(&self) -> u16 {
        self.port
    }
    fn bind_mode(&self) -> BindMode {
        BindMode::Specific(LOCALHOST)
    }
    async fn on_packet(&self, data: &[u8], _socket: &Arc<UdpSocket>, _peer: SocketAddr) {
        black_box(data);
        let p = &self.progress;
        let received = p.received.fetch_add(1, Ordering::Relaxed) + 1;
        if received >= p.target.load(Ordering::Relaxed) {
            p.reached.notify_one();
        }
    }
}

async fn client_socket(port: u16) -> UdpSocket {
    let socket = UdpSocket::bind((LOCALHOST, 0)).await.expect("bind client");
    socket
        .connect((LOCALHOST, port))
        .await
        .expect("connect client");
    socket
}

fn bench_echo(c: &mut Criterion) {
    let port = free_port();
    let _server = Server::start(Echo { port });
    let rt = client_runtime();
    let socket = Arc::new(rt.block_on(client_socket(port)));

    // Wait until the server answers. Before it binds, the ICMP "port
    // unreachable" surfaces on this connected socket as ConnectionRefused.
    wait_until("UDP echo server", || {
        rt.block_on(async {
            let _ = socket.send(b"ping").await;
            let mut buf = [0u8; 4];
            matches!(
                tokio::time::timeout(Duration::from_millis(50), socket.recv(&mut buf)).await,
                Ok(Ok(_))
            )
        })
    });

    // Drop late replies to extra probes so they can't pair with a benchmark request.
    std::thread::sleep(Duration::from_millis(100));
    let mut scratch = [0u8; 64];
    while socket.try_recv(&mut scratch).is_ok() {}

    let mut group = c.benchmark_group("udp/echo_roundtrip");
    for size in [64usize, 1024, 8 * 1024] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.to_async(&rt).iter_custom(|iters| {
                let socket = socket.clone();
                async move {
                    let msg = vec![7u8; size];
                    let mut buf = vec![0u8; size];
                    let start = Instant::now();
                    for _ in 0..iters {
                        socket.send(&msg).await.expect("send");
                        socket.recv(&mut buf).await.expect("recv");
                    }
                    let elapsed = start.elapsed();
                    black_box(buf);
                    elapsed
                }
            });
        });
    }
    group.finish();
}

fn bench_ingest(c: &mut Criterion) {
    let port = free_port();
    let progress = Arc::new(Progress::default());
    let _server = Server::start(Sink {
        port,
        progress: progress.clone(),
    });
    let rt = client_runtime();

    let max_clients = cores();
    let clients: Vec<Arc<UdpSocket>> = rt.block_on(async {
        let mut sockets = Vec::with_capacity(max_clients);
        for _ in 0..max_clients {
            sockets.push(Arc::new(client_socket(port).await));
        }
        sockets
    });

    // Wait until the server receives.
    wait_until("UDP sink server", || {
        let _ = rt.block_on(clients[0].send(&PAYLOAD)); // may be refused until bound
        progress.received.load(Ordering::Relaxed) > 0
    });

    let mut group = c.benchmark_group("udp/ingest");
    group.sample_size(30);
    let mut counts: Vec<usize> = vec![1, max_clients.div_ceil(2), max_clients];
    counts.dedup();
    for n in counts {
        group.throughput(Throughput::Elements(n as u64 * BATCH));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.to_async(&rt).iter_custom(|iters| {
                let clients = clients[..n].to_vec();
                let progress = progress.clone();
                async move { ingest(&clients, &progress, iters).await }
            });
        });
    }
    group.finish();
}

/// Sends `iters` rounds of `BATCH` datagrams from every client, waiting after
/// each round until the server has handled all of them.
async fn ingest(clients: &[Arc<UdpSocket>], progress: &Progress, iters: u64) -> Duration {
    let per_round = clients.len() as u64 * BATCH;
    let mut lost = 0;
    let start = Instant::now();
    for _ in 0..iters {
        let target = progress.received.load(Ordering::Relaxed) + per_round;
        progress.target.store(target, Ordering::Relaxed);

        let mut senders = JoinSet::new();
        for socket in clients {
            let socket = socket.clone();
            senders.spawn(async move {
                for _ in 0..BATCH {
                    socket.send(&PAYLOAD).await.expect("send");
                }
            });
        }
        senders.join_all().await;

        // Wait for the server. If datagrams were dropped the target is never
        // reached; give up after a stall and count them as lost.
        while progress.received.load(Ordering::Relaxed) < target {
            if tokio::time::timeout(Duration::from_millis(100), progress.reached.notified())
                .await
                .is_err()
            {
                lost += target - progress.received.load(Ordering::Relaxed);
                break;
            }
        }
    }
    let elapsed = start.elapsed();
    if lost > 0 {
        eprintln!("warning: {lost} datagrams lost; this sample includes 100 ms stalls");
    }
    elapsed
}

criterion_group!(benches, bench_echo, bench_ingest);
criterion_main!(benches);
