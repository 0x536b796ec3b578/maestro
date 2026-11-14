//! TCP benchmarks: latency and throughput through a real Maestro server on loopback.
//!
//! ```sh
//! cargo bench --bench tcp                       # everything
//! cargo bench --bench tcp -- connect            # filter by name
//! cargo bench --bench tcp -- --save-baseline before
//! cargo bench --bench tcp -- --baseline before  # compare after a change
//! ```
//!
//! HTML reports end up in `target/criterion/report/index.html`.

mod common;

use common::{LOCALHOST, Server, client_runtime, cores, free_port, wait_for_tcp};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use maestro_rs::{BindMode, TcpHandler};
use std::{hint::black_box, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinSet,
    time::Instant,
};

/// Echoes everything back until the client closes the connection.
struct Echo {
    port: u16,
}

impl TcpHandler for Echo {
    fn name(&self) -> &str {
        "bench-tcp-echo"
    }
    fn port(&self) -> u16 {
        self.port
    }
    fn bind_mode(&self) -> BindMode {
        BindMode::Specific(LOCALHOST)
    }
    async fn on_connection(&self, mut stream: TcpStream, _peer: SocketAddr) {
        let _ = stream.set_nodelay(true);
        let mut buf = vec![0u8; 64 * 1024];
        while let Ok(n @ 1..) = stream.read(&mut buf).await {
            if stream.write_all(&buf[..n]).await.is_err() {
                break;
            }
        }
    }
}

fn start_echo() -> (Server, u16) {
    let port = free_port();
    let server = Server::start(Echo { port });
    wait_for_tcp(port);
    (server, port)
}

async fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect((LOCALHOST, port))
        .await
        .expect("connect");
    stream.set_nodelay(true).expect("nodelay");
    // Close with RST instead of FIN: otherwise every iteration leaves a
    // client socket in TIME_WAIT, and long runs would run out of ephemeral
    // ports (especially on macOS and Windows).
    socket2::SockRef::from(&stream)
        .set_linger(Some(Duration::ZERO))
        .expect("linger");
    stream
}

async fn roundtrip(stream: &mut TcpStream, msg: &[u8], buf: &mut [u8]) {
    stream.write_all(msg).await.expect("write");
    stream
        .read_exact(&mut buf[..msg.len()])
        .await
        .expect("read");
}

/// A full connection lifecycle: connect, one 64-byte echo, close.
/// Exercises accept, per-connection task spawn and handler dispatch.
async fn connection_cycle(port: u16) {
    let mut stream = connect(port).await;
    let mut buf = [0u8; 64];
    roundtrip(&mut stream, &[7; 64], &mut buf).await;
    black_box(buf);
}

fn bench_connect(c: &mut Criterion) {
    let (_server, port) = start_echo();
    let rt = client_runtime();

    c.bench_function("tcp/connect_echo_close", |b| {
        b.to_async(&rt).iter(|| connection_cycle(port));
    });
}

fn bench_concurrent_connections(c: &mut Criterion) {
    let (_server, port) = start_echo();
    let rt = client_runtime();

    let mut group = c.benchmark_group("tcp/concurrent_connections");
    group.sample_size(30);
    for clients in [1, cores(), 4 * cores()] {
        group.throughput(Throughput::Elements(clients as u64));
        group.bench_with_input(BenchmarkId::from_parameter(clients), &clients, |b, &n| {
            b.to_async(&rt).iter_custom(|iters| async move {
                let start = Instant::now();
                for _ in 0..iters {
                    let mut tasks = JoinSet::new();
                    for _ in 0..n {
                        tasks.spawn(connection_cycle(port));
                    }
                    tasks.join_all().await;
                }
                start.elapsed()
            });
        });
    }
    group.finish();
}

fn bench_echo(c: &mut Criterion) {
    let (_server, port) = start_echo();
    let rt = client_runtime();

    let mut group = c.benchmark_group("tcp/echo_roundtrip");
    for size in [64usize, 1024, 16 * 1024] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            // One connection per sample, set up before the clock starts:
            // measures steady-state request/response latency only.
            b.to_async(&rt).iter_custom(|iters| async move {
                let mut stream = connect(port).await;
                let msg = vec![7u8; size];
                let mut buf = vec![0u8; size];
                let start = Instant::now();
                for _ in 0..iters {
                    roundtrip(&mut stream, &msg, &mut buf).await;
                }
                let elapsed = start.elapsed();
                black_box(buf);
                elapsed
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_connect,
    bench_concurrent_connections,
    bench_echo
);
criterion_main!(benches);
