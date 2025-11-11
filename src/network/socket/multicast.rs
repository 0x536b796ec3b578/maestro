use std::{
    io::{Error, ErrorKind, Result},
    net::{IpAddr, Ipv4Addr},
};
use tokio::net::UdpSocket;
use tracing::{debug, error};

/// Joins the provided multicast groups on a given UDP socket.
///
/// Supports both IPv4 and IPv6 multicast addresses.
pub(super) async fn join_multicast_groups(socket: &UdpSocket, groups: &[IpAddr]) -> Result<()> {
    for addr in groups {
        match addr {
            IpAddr::V4(mcast) => {
                if let Err(e) = socket.join_multicast_v4(*mcast, Ipv4Addr::UNSPECIFIED) {
                    let kind = match e.kind() {
                        ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
                        ErrorKind::AddrInUse => ErrorKind::AddrInUse,
                        ErrorKind::AddrNotAvailable => ErrorKind::AddrNotAvailable,
                        ErrorKind::InvalidInput => ErrorKind::InvalidInput,
                        _ => ErrorKind::Other,
                    };

                    error!("Failed to join IPv4 multicast group {}: {:?}", mcast, kind);
                    return Err(Error::new(
                        kind,
                        format!("Failed to join IPv4 multicast group {mcast}"),
                    ));
                } else {
                    debug!("Joined IPv4 multicast group {}", mcast);
                }
            }
            IpAddr::V6(mcast) => {
                if let Err(e) = socket.join_multicast_v6(mcast, 0) {
                    let kind = match e.kind() {
                        ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
                        ErrorKind::AddrInUse => ErrorKind::AddrInUse,
                        ErrorKind::AddrNotAvailable => ErrorKind::AddrNotAvailable,
                        ErrorKind::InvalidInput => ErrorKind::InvalidInput,
                        _ => ErrorKind::Other,
                    };

                    error!("Failed to join IPv6 multicast group {}: {:?}", mcast, kind);
                    return Err(Error::new(
                        kind,
                        format!("Failed to join IPv6 multicast group {mcast}"),
                    ));
                } else {
                    debug!("Joined IPv6 multicast group {}", mcast);
                }
            }
        }
    }
    Ok(())
}
