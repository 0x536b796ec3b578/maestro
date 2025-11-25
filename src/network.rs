#[cfg(feature = "tracing")]
use tracing::{error, info, warn};

use getifaddrs::{Address, getifaddrs, if_nametoindex};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
    sync::Arc,
};
use tokio::{
    net::{TcpListener, UdpSocket},
    task::JoinSet,
};

use crate::{
    Error, Result,
    handler::{TcpHandler, UdpHandler},
};

/// Strategies for binding sockets to network interfaces.
#[derive(Debug, Clone, Copy)]
pub enum BindMode {
    /// Bind to all IP addresses associated with the selected [`NetworkInterface`].
    /// This is the default strategy.
    PreferInterface,
    /// Bind to `0.0.0.0` (IPv4) and `::` (IPv6), listening on all interfaces.
    BindAll,
    /// Bind to a specific, manually provided IP address.
    Specific(IpAddr),
}

/// Represents a local network interface and its associated addresses.
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    /// System name (e.g., "eth0", "lo").
    pub name: String,
    /// OS interface index.
    pub index: u32,
    /// List of assigned IPv4 addresses.
    pub inet: Vec<Ipv4Addr>,
    /// List of assigned IPv6 addresses.
    pub inet6: Vec<Ipv6Addr>,
    /// Optional hardware (MAC) address.
    pub mac: Option<[u8; 6]>,
}

impl NetworkInterface {
    /// Generates a pseudo-random, locally-administered MAC address.
    ///
    /// The address conforms to standard conventions by ensuring the
    /// "locally administered" and "unicast" bits are set appropriately.
    fn generate_mac(&self) -> [u8; 6] {
        let mut mac = [0u8; 6];
        rand::fill(&mut mac);
        mac[0] = (mac[0] & 0b11111110) | 0b00000010; // Set the local/unicast bit to remain realistic
        mac
    }

    /// Assigns a specific MAC address to this interface.
    fn _set_mac(&mut self, mac: [u8; 6]) {
        self.mac = Some(mac)
    }
}

/// Resolves a [`NetworkInterface`] by its system name.
///
/// # Arguments
/// * `name` - The system name of the interface (e.g., "eth0").
///
/// # CLI Integration
/// This implementation was designed to integrate directly with
/// [`clap`](https://docs.rs/clap) argument parsing.
/// By implementing [`FromStr`], [`NetworkInterface`] can be parsed
/// automatically from a string argument using `value_parser!()`.
impl FromStr for NetworkInterface {
    type Err = Error;

    /// Resolves a [`NetworkInterface`] by its system name.
    fn from_str(name: &str) -> Result<Self> {
        if name.trim().is_empty() {
            return Err(Error::InvalidInterfaceName("Empty name provided".into()));
        }

        let index = if_nametoindex(name)?;
        let mut information = Self {
            name: name.to_string(),
            index,
            inet: vec![],
            inet6: vec![],
            mac: None,
        };

        for iface in getifaddrs()? {
            if iface.name == name {
                match iface.address {
                    Address::V4(v4) => information.inet.push(v4.address),
                    Address::V6(v6) => information.inet6.push(v6.address),
                    Address::Mac(mac) => information.mac = Some(mac),
                }
            }
        }

        if information.inet.is_empty() && information.inet6.is_empty() {
            return Err(Error::InterfaceNotFound(name.to_string()));
        }

        if information.mac.is_none() {
            information.mac = Some(information.generate_mac())
        }

        Ok(information)
    }
}

/// Internal loop for running a TCP service.
pub async fn run_tcp<H: TcpHandler>(handler: Arc<H>, iface: Arc<NetworkInterface>) -> Result<()> {
    let addrs = resolve_addrs(handler.bind_mode(), handler.port(), &iface);
    let listener = bind_tcp_listener(&addrs)?;

    #[cfg(feature = "tracing")]
    info!(
        "TCP service `{}` started. Listening on {:?} (Interface: {})",
        handler.name(),
        listener.local_addr().map_err(Error::Io)?,
        iface.name
    );

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let h = handler.clone();
                tokio::spawn(async move {
                    h.on_connection(stream, &peer).await;
                });
            }
            Err(e) => {
                #[cfg(feature = "tracing")]
                error!("TCP accept failed for `{}`: {:?}", handler.name(), e);

                #[cfg(not(feature = "tracing"))]
                let _ = e;
            }
        }
    }
}

/// Internal loop for running a UDP service.
pub async fn run_udp<H: UdpHandler>(handler: Arc<H>, iface: Arc<NetworkInterface>) -> Result<()> {
    let addrs = resolve_addrs(handler.bind_mode(), handler.port(), &iface);
    let sockets = bind_udp_sockets(&addrs, &iface, handler.multicast_addrs())?;

    if sockets.is_empty() {
        return Err(Error::NoAddrAvailable);
    }

    #[cfg(feature = "tracing")]
    info!(
        "UDP service `{}` started. Sharded across {} sockets on interface `{}`",
        handler.name(),
        sockets.len(),
        iface.name
    );

    let mut set = JoinSet::new();

    for socket in sockets {
        let h = handler.clone();
        let s = Arc::new(socket);

        set.spawn(async move {
            let mut buf = vec![0u8; 65535];
            loop {
                match s.recv_from(&mut buf).await {
                    Ok((n, peer)) => {
                        h.on_packet(&buf[..n], s.clone(), &peer).await;
                    }
                    Err(e) => {
                        #[cfg(feature = "tracing")]
                        error!("UDP recv critical failure in `{}`: {:?}", h.name(), e);

                        #[cfg(not(feature = "tracing"))]
                        let _ = e;

                        break;
                    }
                }
            }
        });
    }

    while set.join_next().await.is_some() {}
    Ok(())
}

// Socket Helpers
fn resolve_addrs(mode: BindMode, port: u16, iface: &NetworkInterface) -> Vec<SocketAddr> {
    match mode {
        BindMode::Specific(ip) => vec![SocketAddr::new(ip, port)],
        BindMode::BindAll => vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
        ],
        BindMode::PreferInterface => {
            let mut addrs = Vec::new();
            for ip in &iface.inet {
                addrs.push(SocketAddr::new(IpAddr::V4(*ip), port));
            }
            for ip in &iface.inet6 {
                addrs.push(SocketAddr::new(IpAddr::V6(*ip), port));
            }
            if addrs.is_empty() {
                #[cfg(feature = "tracing")]
                warn!(
                    "Interface `{}` has no IPs configured. Falling back to wildcard 0.0.0.0:{}",
                    iface.name, port
                );
                addrs.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port));
            }
            addrs
        }
    }
}

fn bind_tcp_listener(addrs: &[SocketAddr]) -> Result<TcpListener> {
    for addr in addrs {
        let domain = if addr.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        };
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

        socket.set_reuse_address(true)?;
        #[cfg(target_os = "linux")]
        socket.set_reuse_port(true)?;

        if addr.is_ipv6() {
            socket.set_only_v6(true)?;
        }

        if socket.bind(&((*addr).into())).is_ok() {
            socket.listen(1024)?;
            socket.set_nonblocking(true)?;
            return Ok(TcpListener::from_std(socket.into())?);
        }
    }

    Err(Error::NoAddrAvailable)
}

fn bind_udp_sockets(
    addrs: &[SocketAddr],
    iface: &NetworkInterface,
    mcast: &[IpAddr],
) -> Result<Vec<UdpSocket>> {
    let mut sockets = Vec::new();
    let num_cores = num_cpus::get();

    for addr in addrs {
        for _ in 0..num_cores {
            let domain = if addr.is_ipv4() {
                Domain::IPV4
            } else {
                Domain::IPV6
            };
            let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

            socket.set_reuse_address(true)?;
            #[cfg(target_os = "linux")]
            socket.set_reuse_port(true)?;

            let _ = socket.set_recv_buffer_size(7 * 1024 * 1024);
            let _ = socket.set_send_buffer_size(7 * 1024 * 1024);

            if addr.is_ipv6() {
                socket.set_only_v6(true)?;
            } else {
                socket.set_broadcast(true)?;
            }

            if socket.bind(&((*addr).into())).is_ok() {
                socket.set_nonblocking(true)?;
                let udp = UdpSocket::from_std(socket.into())?;

                for group in mcast {
                    join_multicast(&udp, group, iface);
                }
                sockets.push(udp);
            }
        }
    }

    Ok(sockets)
}

fn join_multicast(socket: &UdpSocket, group: &IpAddr, iface: &NetworkInterface) {
    let _ = match group {
        IpAddr::V4(g) => {
            let i = iface.inet.first().cloned().unwrap_or(Ipv4Addr::UNSPECIFIED);
            socket.join_multicast_v4(*g, i)
        }
        IpAddr::V6(g) => socket.join_multicast_v6(g, iface.index),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_resolution() {
        // "lo" should always exist on linux/mac/windows
        let iface = NetworkInterface::from_str("lo");
        assert!(iface.is_ok());
    }

    #[test]
    fn test_resolve_addrs() {
        let iface = NetworkInterface::from_str("lo").unwrap();
        let addrs = resolve_addrs(BindMode::PreferInterface, 8080, &iface);
        assert!(!addrs.is_empty());
    }
}
