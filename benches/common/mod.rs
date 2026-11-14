//! Shared harness for the network benchmarks.
//!
//! The server under test runs on its **own** multi-threaded Tokio runtime in a
//! background thread, like a separate process would. The benchmark client uses
//! a second runtime, so client work isn't scheduled on the server's workers.
#![allow(dead_code)] // each bench binary uses a different subset

use maestro_rs::{NetworkInterface, Service, Supervisor};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, sync::oneshot};

pub(crate) const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// A Maestro supervisor running in the background. Stops when dropped.
pub(crate) struct Server {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Starts a supervisor on the loopback interface running `service`.
    pub(crate) fn start<K, S>(service: S) -> Self
    where
        S: Service<K> + Send + 'static,
    {
        let (stop, stopped) = oneshot::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("maestro-bench-server".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("maestro-server")
                    .build()
                    .expect("server runtime");
                runtime.block_on(async {
                    let iface = NetworkInterface::loopback().expect("loopback interface");
                    let mut supervisor =
                        Supervisor::new(iface).with_shutdown_timeout(Duration::from_millis(200));
                    supervisor.add(service);
                    supervisor
                        .run_until(async {
                            let _ = stopped.await;
                        })
                        .await
                        .expect("server failed");
                });
            })
            .expect("spawn server thread");
        Self {
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Runtime for the benchmark client.
pub(crate) fn client_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("bench-client")
        .build()
        .expect("client runtime")
}

/// A port that was free a moment ago.
pub(crate) fn free_port() -> u16 {
    std::net::TcpListener::bind((LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

/// Blocks until `ready()` returns true, or panics after 10 s.
pub(crate) fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "{what} did not become ready");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Blocks until a TCP server accepts connections on `port`.
pub(crate) fn wait_for_tcp(port: u16) {
    wait_until("TCP server", || {
        std::net::TcpStream::connect(SocketAddr::new(LOCALHOST, port)).is_ok()
    });
}

/// Number of CPU cores, used to size concurrent benchmarks.
pub(crate) fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}
