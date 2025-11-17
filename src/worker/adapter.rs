use std::sync::Arc;

use crate::{
    NetworkInterface, TcpHandler, UdpHandler,
    runtime::{Runtime, Tcp, TcpRuntime, Udp, UdpRuntime},
};

/// Converts a protocol-specific handler into a concrete [`Runtime`] implementation.
///
/// This trait bridges low-level protocol handlers (like [`TcpHandler`] and [`UdpHandler`])
/// with the unified [`Runtime`] abstraction.
///
/// Implemented automatically for all compatible handler types.
pub trait WorkerAdapter<P> {
    /// The resulting [`Runtime`] type produced by this adapter.
    type RuntimeType: Runtime + Send + Sync + 'static;

    /// Wraps the handler into a [`Runtime`] bound to the given [`NetworkInterface`].
    fn into_worker(self, network_interface: Arc<NetworkInterface>) -> Self::RuntimeType;
}

impl<R> WorkerAdapter<Tcp> for R
where
    R: TcpHandler + Send + Sync + 'static,
{
    type RuntimeType = TcpRuntime<R>;

    fn into_worker(self, network_interface: Arc<NetworkInterface>) -> Self::RuntimeType {
        TcpRuntime {
            inner: Arc::new(self),
            network_interface,
        }
    }
}

impl<R> WorkerAdapter<Udp> for R
where
    R: UdpHandler + Send + Sync + 'static,
{
    type RuntimeType = UdpRuntime<R>;

    fn into_worker(self, network_interface: Arc<NetworkInterface>) -> Self::RuntimeType {
        UdpRuntime {
            inner: Arc::new(self),
            network_interface,
        }
    }
}
