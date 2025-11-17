pub mod tcp;
pub mod udp;

use async_trait::async_trait;
use std::{io::Result, net::IpAddr, sync::Arc};

use crate::{
    NetworkInterface, TcpHandler, UdpHandler,
    network::socket::bind::{BindMode, bind_tcp_listener, bind_udp_sockets},
    runtime::{tcp::run_tcp_service, udp::run_udp_service},
};

/// Marker type representing the TCP protocol.
///
/// Used with [`WorkerAdapter`] to associate a handler with a TCP service.
pub struct Tcp;
/// Marker type representing the UDP protocol.
///
/// Used with [`WorkerAdapter`] to associate a handler with a UDP service.
pub struct Udp;

/// Defines a network service that can be started, supervised, and gracefully stopped.
///
/// The [`Runtime`] trait abstracts protocol-specific implementations (TCP/UDP),
/// unifying them under a consistent async service interface. Supervisors
/// (`WorkerRuntime`) manage the lifecycle of these runtimes.
///
/// Implementors should provide networking behavior through `serve`,
/// typically by delegating to a protocol handler.
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Returns a static name for this runtime (used for logs and metrics).
    fn name(&self) -> &'static str;

    /// Returns the port this runtime should listen on.
    fn port(&self) -> u16;

    /// Returns the [`NetworkInterface`] this runtime is bound to.
    fn network_interface(&self) -> &NetworkInterface;

    /// Returns how the runtime should bind to interfaces.
    ///
    /// Defaults to [`BindMode::PreferInterface`].
    fn bind_mode(&self) -> BindMode {
        BindMode::PreferInterface
    }

    /// Returns multicast addresses used by the runtime.
    ///
    /// Defaults to an empty list.
    fn multicast_addrs(&self) -> &[IpAddr] {
        &[]
    }

    /// Runs the runtime until it completes or fails.
    ///
    /// Implementations are expected to handle their protocol logic and return
    /// an [`std::io::Result`] when the service ends.
    async fn serve(&self) -> Result<()>;
}

/// TCP service adapter that wraps a [`TcpHandler`] implementation.
///
/// This adapter handles binding, accepting connections, and delegating
/// handling to the inner [`TcpHandler`].
pub struct TcpRuntime<R> {
    pub(crate) inner: Arc<R>,
    pub(crate) network_interface: Arc<NetworkInterface>,
}

/// UDP service adapter that wraps a [`UdpHandler`] implementation.
///
/// This adapter manages socket binding and delegates packet handling
/// to the inner [`UdpHandler`].
pub struct UdpRuntime<R> {
    pub(crate) inner: Arc<R>,
    pub(crate) network_interface: Arc<NetworkInterface>,
}

#[async_trait]
impl<R> Runtime for TcpRuntime<R>
where
    R: TcpHandler + Send + Sync + 'static,
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

    /// Runs the TCP runtime by binding a listener and serving incoming connections.
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
impl<R> Runtime for UdpRuntime<R>
where
    R: UdpHandler + Send + Sync + 'static,
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
        self.inner.multicast_addrs()
    }

    fn bind_mode(&self) -> BindMode {
        self.inner.bind_mode()
    }

    /// Runs the UDP runtime by binding sockets and processing datagrams.
    async fn serve(&self) -> Result<()> {
        let sockets = bind_udp_sockets(self, Arc::clone(&self.network_interface)).await?;
        run_udp_service(Arc::clone(&self.inner), sockets).await
    }
}
