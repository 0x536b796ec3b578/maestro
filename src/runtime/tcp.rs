use async_trait::async_trait;
use std::{io::Result, net::SocketAddr, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info};

use crate::{NetworkInterface, network::socket::bind::BindMode};

/// Defines the interface for handling TCP connections.
///
/// A `TcpHandler` describes how a runtime should behave when a new
/// TCP connection is accepted. It is wrapped by [`TcpRuntime`]
/// to create a fully supervised, restartable network service.
///
/// Users typically implement this trait for their TCP service logic.
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

/// Runs the connection-accept loop for a [`TcpHandler`].
///
/// This function:
/// - Binds a [`TcpListener`] to the configured address.
/// - Accepts incoming connections in a loop.
/// - Spawns a new asynchronous task for each connection,
///   invoking [`TcpHandler::on_connection`].
pub(crate) async fn run_tcp_service<R>(
    runtime: Arc<R>,
    listener: TcpListener,
    network_interface: Arc<NetworkInterface>,
) -> Result<()>
where
    R: TcpHandler + Send + Sync + Sized + 'static,
{
    info!(
        "TCP service `{}` listening on {:?}",
        runtime.name(),
        listener.local_addr()?
    );

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(connection) => connection,
            Err(e) => {
                error!("{}: accept failed: {:?}", runtime.name(), e);
                continue;
            }
        };

        let runtime = Arc::clone(&runtime);
        let network_interface = Arc::clone(&network_interface);

        tokio::spawn(async move {
            runtime
                .on_connection(stream, &peer, &network_interface)
                .await;
        });
    }
}
