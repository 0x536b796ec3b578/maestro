use async_trait::async_trait;
use std::{
    io::{Error, ErrorKind, Result},
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::{net::UdpSocket, task::JoinSet};
use tokio_util::bytes::BytesMut;
use tracing::{error, info};

use crate::network::socket::bind::BindMode;

const BUFFER_SIZE: usize = 512;

/// Defines the interface for handling UDP packets.
///
/// A `UdpHandler` processes incoming datagrams and may optionally
/// respond using the same socket. It is wrapped by [`UdpRuntime`]
/// to form a supervised, restartable network service.
///
/// Users typically implement this trait for their UDP service logic.
#[async_trait]
pub trait UdpHandler: Send + Sync {
    /// Returns a static name for this handler (used in logs and metrics).
    fn name(&self) -> &'static str;

    /// Returns the UDP port on which this handler should listen.
    fn port(&self) -> u16;

    /// Returns the binding strategy for this handler.
    ///
    /// Defaults to [`BindMode::PreferInterface`].
    fn bind_mode(&self) -> BindMode {
        BindMode::PreferInterface
    }

    /// Returns multicast addresses the handler should join.
    ///
    /// Defaults to an empty slice.
    fn multicast_addrs(&self) -> &[IpAddr] {
        &[]
    }

    /// Called whenever a new UDP datagram is received.
    ///
    /// # Parameters
    /// - `data`: The received payload.
    /// - `socket`: The bound [`UdpSocket`] used to send or receive further packets.
    /// - `peer`: The remote endpoint address.
    async fn on_packet(&self, data: &[u8], socket: Arc<UdpSocket>, peer: &SocketAddr);
}

/// Runs the UDP receive loop for a [`UdpHandler`].
///
/// This function:
/// - Validates that sockets are available for binding.
/// - Spawns a task per socket, listening for incoming packets.
/// - Delegates processing to [`UdpHandler::on_packet`] for each datagram.
pub(crate) async fn run_udp_service<S>(runtime: Arc<S>, sockets: Vec<UdpSocket>) -> Result<()>
where
    S: UdpHandler + Send + Sync + 'static,
{
    if sockets.is_empty() {
        return Err(Error::new(
            ErrorKind::AddrNotAvailable,
            "No UDP sockets available",
        ));
    }

    info!(
        "UDP service `{}` listening on {} sockets",
        runtime.name(),
        sockets.len()
    );

    let mut tasks = JoinSet::new();

    for socket in sockets {
        let runtime = Arc::clone(&runtime);
        let socket = Arc::new(socket);

        tasks.spawn(async move {
            let mut buffer = BytesMut::with_capacity(BUFFER_SIZE);
            buffer.resize(BUFFER_SIZE, 0);

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((byte_count, peer)) => {
                        let data = &buffer[..byte_count];
                        runtime.on_packet(data, Arc::clone(&socket), &peer).await;
                    }
                    Err(e) => {
                        error!("{}: recv_from failed: {:?}", runtime.name(), e);
                        break;
                    }
                }
            }
        });
    }

    while let Some(res) = tasks.join_next().await {
        if let Err(e) = res {
            error!("UDP socket task failed: {:?}", e);
        }
    }

    Ok(())
}
