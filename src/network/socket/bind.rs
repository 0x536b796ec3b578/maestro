use socket2::{Domain, Protocol, Socket, Type};
use std::{
    io::{Error, ErrorKind, Result},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};
use tokio::net::{TcpListener, UdpSocket};
use tracing::{error, info};

use crate::{
    Service,
    network::socket::{
        buffer::{tcp_backlog, tcp_recvbuf_size, tcp_sendbuf_size, udp_recvbuf_size},
        multicast::join_multicast_groups,
    },
    service::BindMode,
};

/// Computes the list of socket addresses to bind for a service.
///
/// - [`BindMode::Specific`] → binds only to the specified address.
/// - [`BindMode::BindAll`] → binds to both IPv4 and IPv6 unspecified addresses.
/// - [`BindMode::PreferInterface`] → binds to all IPs assigned to the
///   service’s [`crate::NetworkInterface`].
///
/// If no IPs are available on the interface, falls back to `0.0.0.0:<port>`.
pub(crate) fn bind_addresses<S: Service>(service: &S) -> Vec<SocketAddr> {
    match service.bind_mode() {
        BindMode::Specific(ip) => vec![SocketAddr::new(ip, service.port())],
        BindMode::BindAll => vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), service.port()),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), service.port()),
        ],
        BindMode::PreferInterface => {
            let network_interface = service.network_interface();
            let mut addresses =
                Vec::with_capacity(network_interface.inet.len() + network_interface.inet6.len());

            for ip in &network_interface.inet {
                addresses.push(SocketAddr::new(IpAddr::V4(*ip), service.port()));
            }
            for ip in &network_interface.inet6 {
                addresses.push(SocketAddr::new(IpAddr::V6(*ip), service.port()));
            }

            if addresses.is_empty() {
                tracing::warn!(
                    "Interface {} has no IPs. Falling back to '0.0.0.0:{}'.",
                    network_interface.name,
                    service.port()
                );
                addresses.push(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    service.port(),
                ));
            }

            addresses
        }
    }
}

/// Binds a single TCP listener for the given service.
///
/// Tries each computed bind address in order until one succeeds.
/// Configures platform-specific options such as `SO_REUSEADDR` and `SO_REUSEPORT`
/// (when available).
pub(crate) async fn bind_tcp_listener<S: Service>(service: &S) -> Result<TcpListener> {
    for address in bind_addresses(service) {
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

                #[cfg(target_os = "linux")]
                if let Some(size) = tcp_recvbuf_size() {
                    socket.set_recv_buffer_size(size)?;
                }

                #[cfg(target_os = "linux")]
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
                info!("TCP service `{}` bound on {}", service.name(), address);

                return Ok(listener);
            }
            Err(e) => error!("Failed to create TCP socket for {}: {:?}", address, e),
        }
    }

    Err(Error::new(
        ErrorKind::AddrNotAvailable,
        format!(
            "No valid TCP address could be bound for {}.",
            service.name()
        ),
    ))
}

/// Binds UDP sockets for the given service and joins multicast groups if applicable.
///
/// For each valid bind address:
/// - Configures socket options such as `SO_REUSEADDR`, `SO_BROADCAST`, and `SO_REUSEPORT` (Linux).
/// - Binds and wraps the socket in a non-blocking [`UdpSocket`].
/// - Joins multicast groups returned by [`Service::multicast_addrs`].
pub(crate) async fn bind_udp_sockets<S: Service>(service: &S) -> Result<Vec<UdpSocket>> {
    let mut sockets = Vec::new();

    for address in bind_addresses(service) {
        let domain = match address {
            SocketAddr::V4(_) => Domain::IPV4,
            SocketAddr::V6(_) => Domain::IPV6,
        };

        match Socket::new(domain, Type::DGRAM, Some(Protocol::UDP)) {
            Ok(socket) => {
                socket.set_reuse_address(true)?;
                #[cfg(target_os = "linux")]
                socket.set_reuse_port(true)?;

                if address.is_ipv6() {
                    socket.set_only_v6(true)?;
                } else {
                    socket.set_broadcast(true)?;
                }

                #[cfg(target_os = "linux")]
                if let Some(size) = udp_recvbuf_size() {
                    socket.set_recv_buffer_size(size)?;
                }

                if let Err(e) = socket.bind(&address.into()) {
                    error!("Failed to bind {}: {:?}", address, e);
                    continue;
                }

                socket.set_nonblocking(true)?;

                let socket = UdpSocket::from_std(socket.into())?;
                info!("UDP service `{}` bound on {}", service.name(), address);

                if !service.multicast_addrs().is_empty()
                    && let Err(e) = join_multicast_groups(&socket, service.multicast_addrs()).await
                {
                    error!("Failed to join multicast groups on {}: {:?}", address, e);
                    continue;
                }

                sockets.push(socket);
            }
            Err(e) => error!("Failed to create UDP socket for {}: {:?}", address, e),
        }
    }

    if sockets.is_empty() {
        return Err(Error::new(
            ErrorKind::AddrNotAvailable,
            format!("No valid UDP socket could be bound for {}.", service.name()),
        ));
    }

    Ok(sockets)
}
