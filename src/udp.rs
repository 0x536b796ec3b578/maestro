use std::{
    future::Future,
    io::ErrorKind,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use tokio::{net::UdpSocket, task::JoinSet};
use tokio_util::sync::CancellationToken;

use crate::{
    BindMode, Result, bind,
    log::{log_debug, log_info},
    service::Context,
};

/// Largest possible UDP payload.
const MAX_DATAGRAM: usize = 65_535;

/// A UDP service.
///
/// Datagrams are handled on every CPU core:
///
/// - **Linux**: each unicast address gets one `SO_REUSEPORT` socket per core,
///   and the kernel spreads peers across them. Each socket has one receive
///   loop, so datagrams from the same peer are handled in order.
/// - **macOS, Windows, BSD**: one socket per address, read by one receive loop
///   per core. Datagrams from the same peer may be handled concurrently and
///   out of order.
///
/// A receive loop awaits [`on_packet`](Self::on_packet) before reading its
/// next datagram, so spawn a task inside `on_packet` for slow work.
///
/// # Examples
///
/// ```
/// use maestro_rs::UdpHandler;
/// use std::{net::SocketAddr, sync::Arc};
/// use tokio::net::UdpSocket;
///
/// struct Echo;
///
/// impl UdpHandler for Echo {
///     fn name(&self) -> &str { "echo" }
///     fn port(&self) -> u16 { 7 }
///
///     async fn on_packet(&self, data: &[u8], socket: &Arc<UdpSocket>, peer: SocketAddr) {
///         let _ = socket.send_to(data, peer).await;
///     }
/// }
/// ```
pub trait UdpHandler: Send + Sync + 'static {
    /// Service name used in logs.
    fn name(&self) -> &str;

    /// Port to listen on.
    fn port(&self) -> u16;

    /// Which local unicast addresses to listen on. Defaults to [`BindMode::Interface`].
    fn bind_mode(&self) -> BindMode {
        BindMode::Interface
    }

    /// Multicast groups to join on the supervisor's interface. Defaults to none.
    ///
    /// Each group gets one extra socket on [`port`](Self::port). On Unix that
    /// socket is bound to the group address, so it receives only that group's
    /// traffic.
    fn multicast_groups(&self) -> &[IpAddr] {
        &[]
    }

    /// Handles one datagram.
    ///
    /// `socket` is the socket that received it. Use it to reply, or clone the
    /// `Arc` to keep it after this call returns.
    fn on_packet(
        &self,
        data: &[u8],
        socket: &Arc<UdpSocket>,
        peer: SocketAddr,
    ) -> impl Future<Output = ()> + Send;
}

pub(crate) async fn serve<H: UdpHandler>(handler: Arc<H>, ctx: Context) -> Result<()> {
    serve_with(handler, ctx, bind::udp_layout).await
}

async fn serve_with<H, L>(handler: Arc<H>, ctx: Context, layout: L) -> Result<()>
where
    H: UdpHandler,
    L: Fn(SocketAddr) -> bind::UdpLayout,
{
    let port = handler.port();
    let addrs = bind::unicast_addrs(handler.bind_mode(), port, &ctx.iface)?;

    // Each socket is paired with the number of receive loops to run on it: a
    // sharded reuse-port socket needs one, a single shared socket spreads its
    // work over several. The layout is chosen per address, because only a
    // specific unicast address may be sharded — see `bind::udp_layout`.
    let mut sockets: Vec<(UdpSocket, usize)> = bind::bind_each(&addrs, |addr| {
        let plan = layout(addr);
        (0..plan.sockets)
            .map(|_| bind::udp_socket(addr, plan.reuse_port).map(|socket| (socket, plan.workers)))
            .collect::<Result<Vec<_>>>()
    })?
    .into_iter()
    .flatten()
    .collect();

    // A multicast group socket is the sole receiver of its group, so it is never
    // sharded; it spreads its work over several receive loops like any shared
    // socket.
    for &group in handler.multicast_groups() {
        let socket = bind::multicast_socket(group, port, &ctx.iface)?;
        sockets.push((socket, bind::shared_workers()));
    }

    let receive_loops: usize = sockets.iter().map(|&(_, loops)| loops).sum();
    log_info!(
        service = handler.name(),
        addrs = ?addrs,
        groups = ?handler.multicast_groups(),
        sockets = sockets.len(),
        receive_loops,
        interface = %ctx.iface,
        "UDP listening"
    );

    let mut workers = JoinSet::new();
    for (socket, loops) in sockets {
        let socket = Arc::new(socket);
        for _ in 0..loops {
            workers.spawn(recv_loop(
                socket.clone(),
                handler.clone(),
                ctx.shutdown.clone(),
            ));
        }
    }
    // The first failing worker fails the whole service. Dropping `workers`
    // aborts the rest, and the supervisor restarts the service.
    while let Some(joined) = workers.join_next().await {
        joined??;
    }
    Ok(())
}

async fn recv_loop<H: UdpHandler>(
    socket: Arc<UdpSocket>,
    handler: Arc<H>,
    shutdown: CancellationToken,
) -> Result<()> {
    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        let received = tokio::select! {
            biased;
            () = shutdown.cancelled() => return Ok(()),
            received = socket.recv_from(&mut buf) => received,
        };
        match received {
            Ok((len, peer)) => handler.on_packet(&buf[..len], &socket, peer).await,
            // ICMP errors from earlier sends surface on some platforms. They
            // concern a previous peer, not this socket.
            Err(e) if is_transient(e.kind()) => {
                log_debug!(service = handler.name(), error = %e, "ignoring transient UDP error");
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn is_transient(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::ConnectionReset | ErrorKind::ConnectionRefused | ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::tests::loopback;
    use std::{
        collections::HashSet,
        net::Ipv4Addr,
        sync::Mutex,
        thread::{self, ThreadId},
        time::Duration,
    };

    /// Echoes and records which worker thread handled each datagram.
    struct Echo {
        port: u16,
        threads: Mutex<HashSet<ThreadId>>,
    }

    impl UdpHandler for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn port(&self) -> u16 {
            self.port
        }
        fn bind_mode(&self) -> BindMode {
            BindMode::Specific(Ipv4Addr::LOCALHOST.into())
        }
        async fn on_packet(&self, data: &[u8], socket: &Arc<UdpSocket>, peer: SocketAddr) {
            self.threads.lock().unwrap().insert(thread::current().id());
            // Hold the receive loop briefly so other loops must pick up traffic.
            tokio::time::sleep(Duration::from_millis(5)).await;
            let _ = socket.send_to(data, peer).await;
        }
    }

    /// The layout used on macOS, Windows and BSD: one socket, several loops.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn shared_socket_layout_serves_concurrently() {
        let port = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let handler = Arc::new(Echo {
            port,
            threads: Mutex::default(),
        });
        let ctx = Context {
            iface: Arc::new(loopback()),
            shutdown: CancellationToken::new(),
        };
        // Force the shared-socket layout (one socket, several receive loops) so
        // the test exercises concurrent service on a single socket.
        let layout = |_: SocketAddr| bind::UdpLayout {
            sockets: 1,
            reuse_port: false,
            workers: 4,
        };
        let server = tokio::spawn(serve_with(handler.clone(), ctx.clone(), layout));

        let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        client.connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        let mut replies = 0;
        let mut buf = [0u8; 8];
        while replies < 64 {
            for _ in 0..16 {
                // Until the server is bound, ICMP "port unreachable" surfaces
                // here as ConnectionRefused; just keep trying.
                let _ = client.send(b"ping").await;
            }
            for _ in 0..16 {
                match tokio::time::timeout(Duration::from_millis(500), client.recv(&mut buf)).await
                {
                    Ok(Ok(4)) => replies += 1,
                    _ => break, // server may still be binding; resend
                }
            }
        }

        ctx.shutdown.cancel();
        server.await.unwrap().unwrap();
        assert!(
            handler.threads.lock().unwrap().len() > 1,
            "datagrams were handled by a single receive loop"
        );
    }
}
