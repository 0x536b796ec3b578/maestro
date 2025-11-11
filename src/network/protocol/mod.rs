mod tcp;
mod udp;

pub use tcp::TcpHandler;
pub use udp::UdpHandler;

pub(crate) use tcp::run_tcp_service;
pub(crate) use udp::run_udp_service;

/// Marker type representing the TCP protocol.
///
/// Used with ServiceAdapter`
/// to associate a handler with a TCP service.
pub struct Tcp;
/// Marker type representing the UDP protocol.
///
/// Used with `ServiceAdapter`
/// to associate a handler with a UDP service.
pub struct Udp;
