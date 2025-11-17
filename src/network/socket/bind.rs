use socket2::{Domain, Protocol, Socket, Type};
use std::{
    io::{Error, ErrorKind, Result},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tokio::net::{TcpListener, UdpSocket};
use tracing::{debug, error, info};

use crate::{
    NetworkInterface,
    network::socket::{
        buffer::{tcp_backlog, tcp_recvbuf_size, tcp_sendbuf_size, udp_recvbuf_size},
        multicast::join_multicast_groups,
    },
    runtime::Runtime,
};

/// Determines how a runtime selects the IP addresses on which it binds sockets.
///
/// The bind strategy affects both TCP and UDP runtimes, and controls which
/// address(es) are attempted when creating listeners or datagram sockets.
///
/// # Variants
/// - [`BindMode::PreferInterface`] (default)
///   Use all IP addresses configured on the runtime’s [`NetworkInterface`].
///
/// - [`BindMode::BindAll`]
///   Bind to the unspecified IPv4 and IPv6 wildcard addresses (`0.0.0.0` and `::`).
///
/// - [`BindMode::Specific`]
///   Force binding to a single, explicit IP address.
#[derive(Debug, Clone)]
pub enum BindMode {
    /// Bind to all addresses assigned to the runtime’s network interface.
    PreferInterface,
    /// Bind to IPv4 and IPv6 wildcard addresses (`0.0.0.0` and `[::]`).
    BindAll,
    /// Bind only to the specified IP address.
    Specific(IpAddr),
}

/// Computes the list of socket addresses that a runtime should attempt to bind.
///
/// This function expands the [`BindMode`] strategy into one or more concrete
/// [`SocketAddr`]s. The returned addresses are ordered, and callers attempt them
/// sequentially until one succeeds.
///
/// - [`BindMode::Specific`]: Returns exactly one address.
/// - [`BindMode::BindAll`]: Returns wildcard IPv4 + IPv6 addresses.
/// - [`BindMode::PreferInterface`]: Returns all addresses on the
///   runtime’s [`NetworkInterface`].
///
/// If no IPs are available on the interface, falls back to `0.0.0.0:<port>`.
pub(crate) fn bind_addresses<R: Runtime>(runtime: &R) -> Vec<SocketAddr> {
    match runtime.bind_mode() {
        BindMode::Specific(ip) => vec![SocketAddr::new(ip, runtime.port())],
        BindMode::BindAll => vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), runtime.port()),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), runtime.port()),
        ],
        BindMode::PreferInterface => {
            let iface = runtime.network_interface();
            let mut addrs = Vec::with_capacity(iface.inet.len() + iface.inet6.len());

            for ip in &iface.inet {
                addrs.push(SocketAddr::new(IpAddr::V4(*ip), runtime.port()));
            }
            for ip in &iface.inet6 {
                addrs.push(SocketAddr::new(IpAddr::V6(*ip), runtime.port()));
            }

            if addrs.is_empty() {
                tracing::warn!(
                    "Interface {} has no IPs, falling back to 0.0.0.0:{}",
                    iface.name,
                    runtime.port()
                );
                addrs.push(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    runtime.port(),
                ));
            }
            addrs
        }
    }
}

/// Creates and binds a TCP listener for the given runtime.
///
/// The runtime’s [`BindMode`] determines the list of addresses to attempt.
/// The function tries each candidate address in sequence until one succeeds.
///
/// Tries each computed bind address in order until one succeeds.
/// Configures platform-specific options such as `SO_REUSEADDR` and `SO_REUSEPORT`
/// (when available).
pub(crate) async fn bind_tcp_listener<R: Runtime>(runtime: &R) -> Result<TcpListener> {
    for address in bind_addresses(runtime) {
        let domain = match address {
            SocketAddr::V4(_) => Domain::IPV4,
            SocketAddr::V6(_) => Domain::IPV6,
        };

        match Socket::new(domain, Type::STREAM, Some(Protocol::TCP)) {
            Ok(socket) => {
                socket.set_reuse_address(true)?;
                #[cfg(target_os = "linux")]
                socket.set_reuse_port(true)?;
                if address.is_ipv6() {
                    socket.set_only_v6(true)?;
                }

                if let Some(size) = tcp_recvbuf_size() {
                    socket.set_recv_buffer_size(size)?;
                }
                if let Some(size) = tcp_sendbuf_size() {
                    socket.set_send_buffer_size(size)?;
                }

                if let Err(e) = socket.bind(&address.into()) {
                    error!("Failed to bind {}: {:?}", address, e);
                    continue;
                }

                socket.listen(tcp_backlog())?;
                socket.set_nonblocking(true)?;

                let listener = TcpListener::from_std(socket.into())?;
                info!("TCP runtime `{}` bound on {}", runtime.name(), address);
                return Ok(listener);
            }
            Err(e) => error!("Failed to create TCP socket for {}: {:?}", address, e),
        }
    }

    Err(Error::new(
        ErrorKind::AddrNotAvailable,
        "No valid TCP address",
    ))
}

/// Creates, configures, and binds UDP sockets for a runtime.
///
/// A separate socket is created for each computed bind address.
///
/// # Socket Options Applied
/// - `SO_REUSEADDR`
/// - `SO_REUSEPORT` (Linux)
/// - `SO_BROADCAST` for IPv4 sockets
/// - IPv6-only mode for IPv6 sockets
/// - Optional receive-buffer sizing (Linux)
///
/// # Multicast Support
/// After binding, each socket automatically joins all multicast groups
/// returned by [`Runtime::multicast_addrs`] using the runtime’s
/// [`NetworkInterface`].
pub(crate) async fn bind_udp_sockets<R: Runtime>(
    runtime: &R,
    network_interface: Arc<NetworkInterface>,
) -> Result<Vec<UdpSocket>> {
    let mut sockets = Vec::new();

    for addr in bind_addresses(runtime) {
        let domain = match addr {
            SocketAddr::V4(_) => Domain::IPV4,
            SocketAddr::V6(_) => Domain::IPV6,
        };

        let raw = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
        raw.set_reuse_address(true)?;
        #[cfg(target_os = "linux")]
        raw.set_reuse_port(true)?;

        if addr.is_ipv6() {
            raw.set_only_v6(true)?;
        } else {
            raw.set_broadcast(true)?;
        }

        #[cfg(target_os = "linux")]
        if let Some(size) = udp_recvbuf_size() {
            raw.set_recv_buffer_size(size)?;
        }

        raw.bind(&addr.into())?;
        raw.set_nonblocking(true)?;

        let socket = UdpSocket::from_std(raw.into())?;
        info!("UDP service '{}' bound on {}", runtime.name(), addr);

        let groups = runtime.multicast_addrs();
        debug!("Runtime multicast groups: {:?}", groups);
        if !groups.is_empty() {
            join_multicast_groups(&socket, groups, &network_interface).await?;
        }

        sockets.push(socket);
    }

    if sockets.is_empty() {
        return Err(Error::new(
            ErrorKind::AddrNotAvailable,
            format!("No valid UDP sockets bound for {}", runtime.name()),
        ));
    }

    Ok(sockets)
}
