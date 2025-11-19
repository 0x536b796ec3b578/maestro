use std::{
    io::Result,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};
use tokio::net::UdpSocket;
use tracing::{debug, error};

use crate::NetworkInterface;

/// Joins the provided multicast groups on a given UDP socket.
///
/// Supports both IPv4 and IPv6 multicast addresses.
pub(super) async fn join_multicast_groups(
    socket: &UdpSocket,
    groups: &[IpAddr],
    iface: &NetworkInterface,
) -> Result<()> {
    let local_addr = socket.local_addr()?;

    let iface_v4 = iface.inet.first().copied().unwrap_or(Ipv4Addr::UNSPECIFIED);
    let iface_v6 = iface
        .inet6
        .iter()
        .find(|addr| addr.is_unicast_link_local())
        .copied()
        .unwrap_or(Ipv6Addr::UNSPECIFIED);
    let iface_index = iface.index;

    debug!(
        "Joining multicast groups on interface {} (index={}, ipv4={}, ipv6={})",
        iface.name, iface_index, iface_v4, iface_v6
    );

    for group in groups {
        match group {
            IpAddr::V4(mcast) if local_addr.is_ipv4() => {
                if let Err(e) = socket.join_multicast_v4(*mcast, iface_v4) {
                    error!("Failed IPv4 join {} on {}: {:?}", mcast, iface_v4, e);
                    return Err(e);
                }
                debug!("Joined IPv4 multicast group {} on {}", mcast, iface_v4);
            }
            IpAddr::V6(mcast) if local_addr.is_ipv6() => {
                if let Err(e) = socket.join_multicast_v6(mcast, iface_index) {
                    error!(
                        "Failed IPv6 join {} on ifindex {} (lladdr={}): {:?}",
                        mcast, iface_index, iface_v6, e
                    );
                    return Err(e);
                }
                debug!(
                    "Joined IPv6 multicast group {} on {}%{}",
                    mcast, iface_v6, iface_index
                );
            }
            _ => {
                debug!("Skipping multicast group {} (incompatible family)", group);
            }
        }
    }

    Ok(())
}
