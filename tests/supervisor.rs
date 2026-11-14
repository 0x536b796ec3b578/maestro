//! End-to-end tests over the loopback interface.

use maestro_rs::{
    BindMode, Error, NetworkInterface, RestartPolicy, Supervisor, TcpHandler, UdpHandler,
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    sync::oneshot,
    task::JoinHandle,
    time::{sleep, timeout},
};

const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

fn loopback() -> NetworkInterface {
    NetworkInterface::loopback().expect("loopback interface")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind((LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn fast_policy() -> RestartPolicy {
    RestartPolicy::default()
        .with_base_delay(Duration::from_millis(20))
        .with_max_delay(Duration::from_millis(100))
}

/// Starts `supervisor` in the background; send on the returned channel to stop it.
fn spawn(supervisor: Supervisor) -> (oneshot::Sender<()>, JoinHandle<maestro_rs::Result<()>>) {
    let (tx, rx) = oneshot::channel();
    let handle = tokio::spawn(supervisor.run_until(async {
        let _ = rx.await;
    }));
    (tx, handle)
}

async fn connect(port: u16) -> TcpStream {
    timeout(TEST_TIMEOUT, async {
        loop {
            if let Ok(stream) = TcpStream::connect((LOCALHOST, port)).await {
                return stream;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("server never came up")
}

struct TcpEcho {
    port: u16,
    delay: Duration,
}

impl TcpHandler for TcpEcho {
    fn name(&self) -> &str {
        "tcp-echo"
    }
    fn port(&self) -> u16 {
        self.port
    }
    fn bind_mode(&self) -> BindMode {
        BindMode::Specific(LOCALHOST)
    }
    async fn on_connection(&self, mut stream: TcpStream, _peer: SocketAddr) {
        let mut buf = [0u8; 64];
        if let Ok(n @ 1..) = stream.read(&mut buf).await {
            sleep(self.delay).await;
            let _ = stream.write_all(&buf[..n]).await;
        }
    }
}

#[tokio::test]
async fn tcp_echo_roundtrip() {
    let port = free_port();
    let mut supervisor = Supervisor::new(loopback());
    supervisor.add(TcpEcho {
        port,
        delay: Duration::ZERO,
    });
    let (stop, handle) = spawn(supervisor);

    let mut stream = connect(port).await;
    stream.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ping");

    stop.send(()).unwrap();
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn shutdown_lets_in_flight_connections_finish() {
    let port = free_port();
    let mut supervisor = Supervisor::new(loopback());
    supervisor.add(TcpEcho {
        port,
        delay: Duration::from_millis(200),
    });
    let (stop, handle) = spawn(supervisor);

    let mut stream = connect(port).await;
    stream.write_all(b"late").await.unwrap();
    sleep(Duration::from_millis(50)).await;
    stop.send(()).unwrap();

    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"late");
    handle.await.unwrap().unwrap();

    // The listener is closed after shutdown.
    assert!(TcpStream::connect((LOCALHOST, port)).await.is_err());
}

#[tokio::test]
async fn shutdown_timeout_aborts_stuck_connections() {
    let port = free_port();
    let mut supervisor =
        Supervisor::new(loopback()).with_shutdown_timeout(Duration::from_millis(50));
    supervisor.add(TcpEcho {
        port,
        delay: Duration::from_secs(60),
    });
    let (stop, handle) = spawn(supervisor);

    let mut stream = connect(port).await;
    stream.write_all(b"x").await.unwrap();
    sleep(Duration::from_millis(50)).await;
    stop.send(()).unwrap();

    timeout(TEST_TIMEOUT, handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut buf = [0u8; 1];
    assert_eq!(
        stream.read(&mut buf).await.unwrap_or(0),
        0,
        "connection closed"
    );
}

#[tokio::test]
async fn service_restarts_once_port_becomes_free() {
    let blocker = std::net::TcpListener::bind((LOCALHOST, 0)).unwrap();
    let port = blocker.local_addr().unwrap().port();

    let mut supervisor =
        Supervisor::new(loopback()).with_restart_policy(fast_policy().with_unlimited_restarts());
    supervisor.add(TcpEcho {
        port,
        delay: Duration::ZERO,
    });
    let (stop, handle) = spawn(supervisor);

    sleep(Duration::from_millis(100)).await;
    drop(blocker);

    let mut stream = connect(port).await;
    stream.write_all(b"up").await.unwrap();
    let mut buf = [0u8; 2];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"up");

    stop.send(()).unwrap();
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn supervisor_fails_when_every_service_gives_up() {
    struct Unbindable;
    impl TcpHandler for Unbindable {
        fn name(&self) -> &str {
            "unbindable"
        }
        fn port(&self) -> u16 {
            1
        }
        fn bind_mode(&self) -> BindMode {
            // TEST-NET-1: never assigned to a local interface.
            BindMode::Specific(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)))
        }
        async fn on_connection(&self, _: TcpStream, _: SocketAddr) {}
    }

    let mut supervisor =
        Supervisor::new(loopback()).with_restart_policy(fast_policy().with_max_restarts(2));
    supervisor.add(Unbindable);

    let result = timeout(TEST_TIMEOUT, supervisor.run_until(std::future::pending()))
        .await
        .expect("supervisor should give up");
    assert!(matches!(result, Err(Error::AllServicesFailed)));
}

#[tokio::test]
async fn empty_supervisor_returns_immediately() {
    let result = Supervisor::new(loopback())
        .run_until(std::future::pending())
        .await;
    assert!(result.is_ok());
}

struct UdpEcho {
    port: u16,
    groups: Vec<IpAddr>,
}

impl UdpHandler for UdpEcho {
    fn name(&self) -> &str {
        "udp-echo"
    }
    fn port(&self) -> u16 {
        self.port
    }
    fn bind_mode(&self) -> BindMode {
        BindMode::Specific(LOCALHOST)
    }
    fn multicast_groups(&self) -> &[IpAddr] {
        &self.groups
    }
    async fn on_packet(&self, data: &[u8], socket: &Arc<UdpSocket>, peer: SocketAddr) {
        let _ = socket.send_to(data, peer).await;
    }
}

/// Sends `msg` to `target` until an echo arrives (the server may still be starting).
async fn udp_roundtrip(client: &UdpSocket, target: SocketAddr, msg: &[u8]) -> Vec<u8> {
    timeout(TEST_TIMEOUT, async {
        let mut buf = [0u8; 64];
        loop {
            client.send_to(msg, target).await.unwrap();
            if let Ok(Ok((n, _))) =
                timeout(Duration::from_millis(50), client.recv_from(&mut buf)).await
            {
                return buf[..n].to_vec();
            }
        }
    })
    .await
    .expect("no UDP reply")
}

#[tokio::test]
async fn udp_echo_roundtrip() {
    let port = free_port();
    let mut supervisor = Supervisor::new(loopback());
    supervisor.add(UdpEcho {
        port,
        groups: vec![],
    });
    let (stop, handle) = spawn(supervisor);

    let client = UdpSocket::bind((LOCALHOST, 0)).await.unwrap();
    let reply = udp_roundtrip(&client, (LOCALHOST, port).into(), b"ping").await;
    assert_eq!(reply, b"ping");

    stop.send(()).unwrap();
    handle.await.unwrap().unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn udp_multicast_group_receives_datagrams() {
    let port = free_port();
    let group = Ipv4Addr::new(239, 255, 77, 1);
    let mut supervisor = Supervisor::new(loopback());
    supervisor.add(UdpEcho {
        port,
        groups: vec![group.into()],
    });
    let (stop, handle) = spawn(supervisor);

    let client = UdpSocket::bind((LOCALHOST, 0)).await.unwrap();
    socket2::SockRef::from(&client)
        .set_multicast_if_v4(&Ipv4Addr::LOCALHOST)
        .unwrap();
    let reply = udp_roundtrip(&client, (group, port).into(), b"mcast").await;
    assert_eq!(reply, b"mcast");

    stop.send(()).unwrap();
    handle.await.unwrap().unwrap();
}
