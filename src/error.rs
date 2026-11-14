use std::{
    io,
    net::{IpAddr, SocketAddr},
};

/// Errors produced by `maestro-rs`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The interface name is empty or contains a NUL byte.
    #[error("invalid network interface name `{0}`")]
    InvalidInterfaceName(String),

    /// No interface with this name exists on the system.
    #[error("network interface `{0}` not found")]
    InterfaceNotFound(String),

    /// [`BindMode::Interface`](crate::BindMode::Interface) was requested but
    /// the interface has no IP address.
    #[error("network interface `{0}` has no IP address to bind to")]
    NoInterfaceAddress(String),

    /// A socket could not be bound to `addr`.
    #[error("failed to bind {addr}")]
    Bind {
        /// The address that could not be bound.
        addr: SocketAddr,
        /// The underlying OS error.
        #[source]
        source: io::Error,
    },

    /// A UDP socket could not join a multicast group.
    #[error("failed to join multicast group {group}")]
    JoinMulticast {
        /// The multicast group.
        group: IpAddr,
        /// The underlying OS error.
        #[source]
        source: io::Error,
    },

    /// An internal worker task panicked or was cancelled.
    #[error("service worker task failed")]
    Task(#[from] tokio::task::JoinError),

    /// Every registered service stopped after exhausting its
    /// [`RestartPolicy`](crate::RestartPolicy) before shutdown was requested.
    #[error("all services stopped after exhausting their restart policy")]
    AllServicesFailed,

    /// Any other I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Shorthand for `Result<T, maestro_rs::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;
