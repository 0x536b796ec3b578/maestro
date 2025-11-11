use async_trait::async_trait;
use std::{io::Result, net::IpAddr, sync::Arc};

use crate::{
    NetworkInterface,
    network::{
        protocol::{Tcp, TcpHandler, Udp, UdpHandler, run_tcp_service, run_udp_service},
        socket::{bind_tcp_listener, bind_udp_sockets},
    },
};

/// Defines a network service that can be started, supervised, and gracefully stopped.
///
/// This trait abstracts protocol-specific implementations (e.g. TCP, UDP),
/// unifying them under a consistent async service interface.
///
/// Implementors should provide networking behavior through `serve`,
/// typically by delegating to a protocol handler.
///
/// # Example
/// ```rust,no_run
/// use async_trait::async_trait;
/// struct EchoHandler;
///
/// #[async_trait]
/// impl TcpService for EchoHandler {
///     fn name(&self) -> &'static str { "echo" }
///     fn port(&self) -> u16 { 8080 }
///     async fn handle_connection(&self, ... ) -> io::Result<()> { ... }
/// }
/// ```
#[async_trait]
pub trait Service: Send + Sync {
    /// Returns a static name for this service (used for logging and metrics).
    fn name(&self) -> &'static str;

    /// Returns the port on which the service should listen.
    fn port(&self) -> u16;

    /// Returns the [`NetworkInterface`] this service is bound to.
    fn network_interface(&self) -> &NetworkInterface;

    /// Returns how the service should bind to network interfaces.
    ///
    /// Defaults to [`BindMode::PreferInterface`].
    fn bind_mode(&self) -> BindMode {
        BindMode::PreferInterface
    }

    /// Returns multicast addresses used by the service.
    ///
    /// Defaults to an empty list.
    fn multicast_addrs(&self) -> &[IpAddr] {
        &[]
    }

    /// Runs the service until it completes or fails.
    ///
    /// Implementations are expected to handle their protocol logic and return
    /// an [`std::io::Result`] when the service ends.
    async fn serve(&self) -> Result<()>;
}

/// Determines how a service binds its sockets to network interfaces.
#[derive(Debug, Clone)]
pub enum BindMode {
    /// Bind using the interface’s primary address (default).
    PreferInterface,
    /// Bind on all available interfaces (`0.0.0.0` or `[::]`).
    BindAll,
    /// Bind to a specific IP address.
    Specific(IpAddr),
}

/// TCP service adapter that wraps a [`TcpHandler`] implementation.
///
/// This adapter handles binding, accepting connections, and delegating
/// handling to the inner [`TcpHandler`].
pub struct TcpService<S> {
    inner: Arc<S>,
    network_interface: Arc<NetworkInterface>,
}

/// UDP service adapter that wraps a [`UdpHandler`] implementation.
///
/// This adapter manages socket binding and delegates packet handling
/// to the inner [`UdpHandler`].
pub struct UdpService<S> {
    inner: Arc<S>,
    network_interface: Arc<NetworkInterface>,
}

#[async_trait]
impl<S> Service for TcpService<S>
where
    S: TcpHandler + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn port(&self) -> u16 {
        self.inner.port()
    }

    fn network_interface(&self) -> &NetworkInterface {
        &self.network_interface
    }

    fn bind_mode(&self) -> BindMode {
        self.inner.bind_mode()
    }

    /// Runs the TCP service by binding a listener and serving incoming connections.
    async fn serve(&self) -> Result<()> {
        let listener = bind_tcp_listener(self).await?;
        run_tcp_service(
            Arc::clone(&self.inner),
            listener,
            Arc::clone(&self.network_interface),
        )
        .await
    }
}

#[async_trait]
impl<S> Service for UdpService<S>
where
    S: UdpHandler + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn port(&self) -> u16 {
        self.inner.port()
    }

    fn network_interface(&self) -> &NetworkInterface {
        &self.network_interface
    }

    fn multicast_addrs(&self) -> &[IpAddr] {
        &[]
    }

    fn bind_mode(&self) -> BindMode {
        self.inner.bind_mode()
    }

    /// Runs the UDP service by binding sockets and processing datagrams.
    async fn serve(&self) -> Result<()> {
        let sockets = bind_udp_sockets(self).await?;
        run_udp_service(
            Arc::clone(&self.inner),
            sockets,
            // Arc::clone(&self.network_interface),
        )
        .await
    }
}

/// Converts a protocol-specific handler into a concrete [`Service`] implementation.
///
/// This trait bridges low-level protocol handlers (like [`TcpHandler`] and [`UdpHandler`])
/// with the unified [`Service`] abstraction.
///
/// Implemented automatically for all compatible handler types.
pub trait ServiceAdapter<P> {
    /// The resulting [`Service`] type produced by this adapter.
    type ServiceType: Service + Send + Sync + 'static;

    /// Wraps the handler into a [`Service`] bound to the given [`NetworkInterface`].
    fn to_service(self, network_interface: Arc<NetworkInterface>) -> Self::ServiceType;
}

impl<S> ServiceAdapter<Tcp> for S
where
    S: TcpHandler + Send + Sync + 'static,
{
    type ServiceType = TcpService<S>;

    fn to_service(self, network_interface: Arc<NetworkInterface>) -> Self::ServiceType {
        TcpService {
            inner: Arc::new(self),
            network_interface,
        }
    }
}

impl<S> ServiceAdapter<Udp> for S
where
    S: UdpHandler + Send + Sync + 'static,
{
    type ServiceType = UdpService<S>;

    fn to_service(self, network_interface: Arc<NetworkInterface>) -> Self::ServiceType {
        UdpService {
            inner: Arc::new(self),
            network_interface,
        }
    }
}
