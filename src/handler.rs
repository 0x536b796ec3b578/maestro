use async_trait::async_trait;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::net::{TcpStream, UdpSocket};

use crate::{
    NetworkInterface, RestartPolicy,
    network::{run_tcp, run_udp},
    supervisor::{SupervisedTask, Task},
};

/// Marker type for TCP service registration.
pub struct Tcp;
/// Marker type for UDP service registration.
pub struct Udp;

/// Defines the behavior of a TCP service.
#[async_trait]
pub trait TcpHandler: Send + Sync + 'static {
    /// Returns the name of the service (used for logs/metrics).
    fn name(&self) -> &'static str;

    /// Returns the port on which the service should listen.
    fn port(&self) -> u16;

    /// Returns the binding strategy. Defaults to [`crate::BindMode::PreferInterface`].
    fn bind_mode(&self) -> crate::BindMode {
        crate::BindMode::PreferInterface
    }

    /// Handles a new incoming TCP connection.
    ///
    /// # Arguments
    /// * `stream` - The connected TCP stream.
    /// * `peer` - The address of the remote peer.
    async fn on_connection(&self, stream: TcpStream, peer: &SocketAddr);
}

/// Defines the behavior of a UDP service.
#[async_trait]
pub trait UdpHandler: Send + Sync + 'static {
    /// Returns the name of the service (used for logs/metrics).
    fn name(&self) -> &'static str;

    /// Returns the port on which the service should listen.
    fn port(&self) -> u16;

    /// Returns the binding strategy. Defaults to [`crate::BindMode::PreferInterface`].
    fn bind_mode(&self) -> crate::BindMode {
        crate::BindMode::PreferInterface
    }
    /// Returns a list of multicast addresses to join. Defaults to empty.
    fn multicast_addrs(&self) -> &[IpAddr] {
        &[]
    }

    /// Handles an incoming UDP packet.
    ///
    /// # Arguments
    /// * `data` - The raw packet data.
    /// * `socket` - The shared socket (thread-safe, can be used to send replies).
    /// * `peer` - The address of the sender.
    async fn on_packet(&self, data: &[u8], socket: Arc<UdpSocket>, peer: &SocketAddr);
}

/// A generic trait to convert user handlers into supervised tasks.
pub trait Service<Kind> {
    /// Consumes the handler and produces a supervised task.
    fn into_task(self, iface: Arc<NetworkInterface>, policy: RestartPolicy) -> Box<dyn Task>;
}

impl<T> Service<Tcp> for T
where
    T: TcpHandler,
{
    fn into_task(self, iface: Arc<NetworkInterface>, policy: RestartPolicy) -> Box<dyn Task> {
        let handler = Arc::new(self);
        Box::new(SupervisedTask::new(handler.name(), policy, move || {
            let h = handler.clone();
            let i = iface.clone();
            Box::pin(async move { run_tcp(h, i).await })
        }))
    }
}

impl<T> Service<Udp> for T
where
    T: UdpHandler,
{
    fn into_task(self, iface: Arc<NetworkInterface>, policy: RestartPolicy) -> Box<dyn Task> {
        let handler = Arc::new(self);
        Box::new(SupervisedTask::new(handler.name(), policy, move || {
            let h = handler.clone();
            let i = iface.clone();
            Box::pin(async move { run_udp(h, i).await })
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    struct MockTcp;
    #[async_trait]
    impl TcpHandler for MockTcp {
        fn name(&self) -> &'static str {
            "MockTcp"
        }
        fn port(&self) -> u16 {
            0
        }
        async fn on_connection(&self, _s: TcpStream, _p: &SocketAddr) {}
    }

    struct MockUdp;
    #[async_trait]
    impl UdpHandler for MockUdp {
        fn name(&self) -> &'static str {
            "MockUdp"
        }
        fn port(&self) -> u16 {
            0
        }
        async fn on_packet(&self, _data: &[u8], _socket: Arc<UdpSocket>, _peer: &SocketAddr) {}
    }

    #[test]
    fn test_tcp_into_task() {
        let iface = Arc::new(NetworkInterface::from_str("lo").unwrap());
        let service = MockTcp;
        let _task = Service::<Tcp>::into_task(service, iface, RestartPolicy::default());
    }

    #[test]
    fn test_udp_into_task() {
        let iface = Arc::new(NetworkInterface::from_str("lo").unwrap());
        let service = MockUdp;
        let _task = Service::<Udp>::into_task(service, iface, RestartPolicy::default());
    }
}
