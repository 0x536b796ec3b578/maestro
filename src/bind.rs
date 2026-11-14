use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
    num::NonZeroUsize,
};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::{TcpListener, UdpSocket};

use crate::{Error, NetworkInterface, Result, log::log_warn};

/// Which local addresses a service listens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BindMode {
    /// Every IP address of the supervisor's [`NetworkInterface`] (default).
    ///
    /// IPv6 link-local addresses are bound with the interface's scope ID.
    /// Fails with [`Error::NoInterfaceAddress`] if the interface has no address.
    #[default]
    Interface,
    /// The unspecified addresses `0.0.0.0` and `[::]`, i.e. every interface.
    Any,
    /// One specific local IP address.
    Specific(IpAddr),
}

const LISTEN_BACKLOG: i32 = 1024;
const UDP_BUFFER_SIZE: usize = 7 * 1024 * 1024;

/// Resolves the unicast socket addresses a service should bind.
pub(crate) fn unicast_addrs(
    mode: BindMode,
    port: u16,
    iface: &NetworkInterface,
) -> Result<Vec<SocketAddr>> {
    let addrs: Vec<_> = match mode {
        BindMode::Interface => iface.addrs().map(|ip| scoped(ip, port, iface)).collect(),
        BindMode::Any => vec![
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
            SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
        ],
        BindMode::Specific(ip) => vec![scoped(ip, port, iface)],
    };
    if addrs.is_empty() {
        return Err(Error::NoInterfaceAddress(iface.name().to_owned()));
    }
    Ok(addrs)
}

/// Attaches the interface scope ID to IPv6 link-local and multicast addresses.
fn scoped(ip: IpAddr, port: u16, iface: &NetworkInterface) -> SocketAddr {
    match ip {
        IpAddr::V6(v6) if v6.is_unicast_link_local() || v6.is_multicast() => {
            SocketAddrV6::new(v6, port, 0, iface.index()).into()
        }
        _ => SocketAddr::new(ip, port),
    }
}

/// Binds every address with `bind`, tolerating partial failure.
///
/// Addresses that fail are logged and skipped. If none succeed, the first error is returned.
pub(crate) fn bind_each<T>(
    addrs: &[SocketAddr],
    mut bind: impl FnMut(SocketAddr) -> Result<T>,
) -> Result<Vec<T>> {
    let mut bound = Vec::with_capacity(addrs.len());
    let mut first_err = None;
    for &addr in addrs {
        match bind(addr) {
            Ok(sock) => bound.push(sock),
            Err(e) => {
                log_warn!(%addr, error = %crate::log::Chain(&e), "skipping address that failed to bind");
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) if bound.is_empty() => Err(e),
        _ => Ok(bound),
    }
}

/// Whether to set `SO_REUSEADDR`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reuse {
    /// Only where it is safe: on Unix it just allows rebinding a port in
    /// `TIME_WAIT`. On Windows it would let another socket silently bind
    /// the same port, so it is skipped there.
    Unix,
    /// Always, so several processes can listen to the same multicast group.
    Always,
}

fn new_socket(addr: SocketAddr, ty: Type, protocol: Protocol, reuse: Reuse) -> io::Result<Socket> {
    let socket = Socket::new(Domain::for_address(addr), ty, Some(protocol))?;
    if cfg!(unix) || reuse == Reuse::Always {
        socket.set_reuse_address(true)?;
    }
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.set_nonblocking(true)?;
    Ok(socket)
}

pub(crate) fn tcp_listener(addr: SocketAddr) -> Result<TcpListener> {
    let bind = || -> io::Result<TcpListener> {
        let socket = new_socket(addr, Type::STREAM, Protocol::TCP, Reuse::Unix)?;
        socket.bind(&addr.into())?;
        socket.listen(LISTEN_BACKLOG)?;
        TcpListener::from_std(socket.into())
    };
    bind().map_err(|source| Error::Bind { addr, source })
}

/// How UDP receive work is spread across cores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UdpLayout {
    /// `SO_REUSEPORT` sockets per unicast address.
    pub(crate) sockets: usize,
    /// Receive loops per socket.
    pub(crate) workers: usize,
}

/// On Linux the kernel load-balances a reuse-port group, so there is one
/// socket per core, each with a single receive loop. Datagrams from one peer
/// stay on one socket and are handled in order. Elsewhere `SO_REUSEPORT`
/// does not balance (or does not exist), so one socket is shared by one
/// receive loop per core instead.
pub(crate) fn udp_layout() -> UdpLayout {
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    if cfg!(target_os = "linux") {
        UdpLayout {
            sockets: cores,
            workers: 1,
        }
    } else {
        UdpLayout {
            sockets: 1,
            workers: cores,
        }
    }
}

pub(crate) fn udp_socket(addr: SocketAddr, reuse_port: bool) -> Result<UdpSocket> {
    udp_socket_with(addr, reuse_port, Reuse::Unix)
}

fn udp_socket_with(addr: SocketAddr, reuse_port: bool, reuse: Reuse) -> Result<UdpSocket> {
    let bind = || -> io::Result<UdpSocket> {
        let socket = new_socket(addr, Type::DGRAM, Protocol::UDP, reuse)?;
        #[cfg(target_os = "linux")]
        if reuse_port {
            socket.set_reuse_port(true)?;
        }
        #[cfg(not(target_os = "linux"))]
        let _ = reuse_port;
        // Large buffers absorb bursts; the OS may clamp them, which is fine.
        let _ = socket.set_recv_buffer_size(UDP_BUFFER_SIZE);
        let _ = socket.set_send_buffer_size(UDP_BUFFER_SIZE);
        if addr.is_ipv4() {
            socket.set_broadcast(true)?;
        }
        socket.bind(&addr.into())?;
        UdpSocket::from_std(socket.into())
    };
    bind().map_err(|source| Error::Bind { addr, source })
}

/// Opens one socket that receives `group` traffic on `iface`.
///
/// On Unix the socket is bound to the group address itself, so it receives only
/// that group's datagrams (not unicast traffic from other interfaces). Windows
/// cannot bind to a multicast address, so there it is bound to the unspecified
/// address. `SO_REUSEADDR` is set on every platform so other listeners of the
/// same group (e.g. the system mDNS responder) can coexist.
pub(crate) fn multicast_socket(
    group: IpAddr,
    port: u16,
    iface: &NetworkInterface,
) -> Result<UdpSocket> {
    let bind_ip = if cfg!(unix) {
        group
    } else if group.is_ipv4() {
        Ipv4Addr::UNSPECIFIED.into()
    } else {
        Ipv6Addr::UNSPECIFIED.into()
    };
    // One socket per group: multicast is copied to every socket in a
    // reuse-port group, so sharding would duplicate each datagram.
    let socket = udp_socket_with(scoped(bind_ip, port, iface), false, Reuse::Always)?;

    let joined = match group {
        IpAddr::V4(g) => {
            let local = iface
                .ipv4()
                .first()
                .copied()
                .unwrap_or(Ipv4Addr::UNSPECIFIED);
            socket.join_multicast_v4(g, local)
        }
        IpAddr::V6(g) => socket.join_multicast_v6(&g, iface.index()),
    };
    joined.map_err(|source| Error::JoinMulticast { group, source })?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::tests::loopback;

    #[test]
    fn interface_mode_uses_every_interface_address() {
        let lo = loopback();
        let addrs = unicast_addrs(BindMode::Interface, 8080, &lo).unwrap();
        assert_eq!(addrs.len(), lo.addrs().count());
        assert!(addrs.contains(&SocketAddr::from((Ipv4Addr::LOCALHOST, 8080))));
    }

    #[test]
    fn any_mode_uses_both_unspecified_addresses() {
        let addrs = unicast_addrs(BindMode::Any, 53, &loopback()).unwrap();
        assert_eq!(
            addrs,
            [
                SocketAddr::from((Ipv4Addr::UNSPECIFIED, 53)),
                SocketAddr::from((Ipv6Addr::UNSPECIFIED, 53)),
            ]
        );
    }

    #[test]
    fn link_local_ipv6_gets_scope_id() {
        let lo = loopback();
        let ip: IpAddr = "fe80::1".parse().unwrap();
        let SocketAddr::V6(addr) = scoped(ip, 1, &lo) else {
            panic!("expected IPv6");
        };
        assert_eq!(addr.scope_id(), lo.index());
    }

    #[test]
    fn bind_each_skips_failures_but_errors_when_all_fail() {
        let addrs = [
            SocketAddr::from(([127, 0, 0, 1], 1)),
            SocketAddr::from(([127, 0, 0, 1], 2)),
        ];
        let ok = bind_each(&addrs, |a| {
            if a.port() == 1 {
                Ok(a)
            } else {
                Err(Error::AllServicesFailed)
            }
        });
        assert_eq!(ok.unwrap(), [addrs[0]]);

        let err = bind_each(&addrs, |_| Err::<(), _>(Error::AllServicesFailed));
        assert!(err.is_err());
    }
}
