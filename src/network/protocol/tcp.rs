use async_trait::async_trait;
use std::{io::Result, net::SocketAddr, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info};

use crate::{BindMode, NetworkInterface};

/// Defines the interface for handling TCP connections.
///
/// A `TcpHandler` describes how a service should behave when a new
/// TCP connection is accepted. It is automatically wrapped by
/// [`TcpHandler`] to create a fully managed,
/// restartable service.
#[async_trait]
pub trait TcpHandler: Send + Sync + Sized {
    /// Returns a static name for this handler (used in logs and metrics).
    fn name(&self) -> &'static str;

    /// Returns the port number on which the service should listen.
    fn port(&self) -> u16;

    /// Returns the preferred socket binding mode.
    ///
    /// Defaults to [`BindMode::PreferInterface`].
    fn bind_mode(&self) -> BindMode {
        BindMode::PreferInterface
    }

    /// Called whenever a new TCP connection is accepted.
    ///
    /// # Parameters
    /// - `stream`: The [`TcpStream`] representing the client connection.
    /// - `peer`: The remote socket address of the client.
    /// - `network_interface`: The [`NetworkInterface`] this service is bound to.
    async fn on_connection(
        &self,
        stream: TcpStream,
        peer: &SocketAddr,
        network_interface: &NetworkInterface,
    );
}

/// Runs a TCP service loop for the given [`TcpHandler`].
///
/// This function:
/// - Binds a [`TcpListener`] to the configured address.
/// - Accepts incoming connections in a loop.
/// - Spawns a new asynchronous task for each connection,
///   invoking [`TcpHandler::on_connection`].
pub(crate) async fn run_tcp_service<S>(
    service: Arc<S>,
    listener: TcpListener,
    network_interface: Arc<NetworkInterface>,
) -> Result<()>
where
    S: TcpHandler + Send + Sync + Sized + 'static,
{
    info!(
        "TCP service `{}` listening on {:?}",
        service.name(),
        listener.local_addr()?
    );

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(connection) => connection,
            Err(e) => {
                error!("{}: accept failed: {:?}", service.name(), e);
                continue;
            }
        };

        let service = Arc::clone(&service);
        let network_interface = Arc::clone(&network_interface);

        tokio::spawn(async move {
            service
                .on_connection(stream, &peer, &network_interface)
                .await;
        });
    }
}
