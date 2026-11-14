use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use crate::{
    BindMode, Result, bind,
    log::{log_debug, log_error, log_info, log_warn},
    service::Context,
};

/// Pause after a failed `accept()` so persistent errors (e.g. out of file
/// descriptors) don't turn into a busy loop.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);

/// A TCP service.
///
/// Each accepted connection is handled in its own task. On shutdown the
/// listeners close and in-flight connections may finish within the
/// supervisor's shutdown timeout. After that they are aborted.
///
/// # Examples
///
/// ```
/// use maestro_rs::TcpHandler;
/// use std::net::SocketAddr;
/// use tokio::{io::AsyncWriteExt, net::TcpStream};
///
/// struct Hello;
///
/// impl TcpHandler for Hello {
///     fn name(&self) -> &str { "hello" }
///     fn port(&self) -> u16 { 8080 }
///
///     async fn on_connection(&self, mut stream: TcpStream, _peer: SocketAddr) {
///         let _ = stream.write_all(b"hello\n").await;
///     }
/// }
/// ```
pub trait TcpHandler: Send + Sync + 'static {
    /// Service name used in logs.
    fn name(&self) -> &str;

    /// Port to listen on.
    fn port(&self) -> u16;

    /// Which local addresses to listen on. Defaults to [`BindMode::Interface`].
    fn bind_mode(&self) -> BindMode {
        BindMode::Interface
    }

    /// Handles one accepted connection.
    fn on_connection(&self, stream: TcpStream, peer: SocketAddr)
    -> impl Future<Output = ()> + Send;
}

pub(crate) async fn serve<H: TcpHandler>(handler: Arc<H>, ctx: Context) -> Result<()> {
    let addrs = bind::unicast_addrs(handler.bind_mode(), handler.port(), &ctx.iface)?;
    let listeners = bind::bind_each(&addrs, bind::tcp_listener)?;

    let mut acceptors = JoinSet::new();
    for listener in listeners {
        log_info!(
            service = handler.name(),
            addr = ?listener.local_addr().ok(),
            interface = %ctx.iface,
            "TCP listening"
        );
        acceptors.spawn(accept_loop(listener, handler.clone(), ctx.shutdown.clone()));
    }
    while let Some(joined) = acceptors.join_next().await {
        joined?;
    }
    Ok(())
}

async fn accept_loop<H: TcpHandler>(
    listener: TcpListener,
    handler: Arc<H>,
    shutdown: CancellationToken,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            Some(joined) = connections.join_next(), if !connections.is_empty() => {
                if let Err(e) = joined {
                    log_error!(service = handler.name(), error = %e, "TCP connection handler panicked");
                }
            }
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let handler = handler.clone();
                    connections.spawn(async move { handler.on_connection(stream, peer).await });
                }
                Err(e) => {
                    log_warn!(service = handler.name(), error = %e, "TCP accept failed");
                    tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                }
            },
        }
    }

    drop(listener);
    if !connections.is_empty() {
        log_debug!(
            service = handler.name(),
            open = connections.len(),
            "draining TCP connections"
        );
    }
    while connections.join_next().await.is_some() {}
}
