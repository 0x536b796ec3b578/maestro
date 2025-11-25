use thiserror::Error;

/// Centralized error type for Maestro.
#[derive(Error, Debug)]
pub enum Error {
    #[error("Network interface '{0}' not found on this system")]
    InterfaceNotFound(String),

    #[error("Invalid interface name: {0}")]
    InvalidInterfaceName(String),

    // #[error("Failed to convert system string (CString error)")]
    // SystemStringError(#[from] std::ffi::NulError),
    #[error("IO operation failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("No valid socket address found for binding")]
    NoAddrAvailable,

    #[error("Service '{0}' failed to start or crashed")]
    ServiceFailure(String),
}

/// Helper alias for `Result<T, maestro_rs::Error>`
pub type Result<T> = std::result::Result<T, Error>;
