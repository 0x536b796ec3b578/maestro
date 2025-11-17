use std::fs;

use tracing::warn;

/// This function determines the backlog from the following sources, in order:
///
/// 1. The `TCP_BACKLOG` environment variable (if set).
/// 2. On Linux, the kernel's `somaxconn` value read from
///    `/proc/sys/net/core/somaxconn`.
/// 3. A portable default of **128**.
///
/// If the resolved value does not fit inside an `i32`, it is clamped to
/// `i32::MAX` and a warning is emitted.
///
/// This function exists to give runtimes a tunable backlog that adapts to the
/// host system while still remaining configurable via environment variables.
pub(super) fn tcp_backlog() -> i32 {
    const DEFAULT: usize = 128;
    #[cfg(target_os = "linux")]
    const SOMAXCONN_PATH: &str = "/proc/sys/net/core/somaxconn";
    #[cfg(not(target_os = "linux"))]
    const SOMAXCONN_PATH: &str = ""; // Unused on non-Linux platforms

    let chosen = env_or_sys("TCP_BACKLOG", SOMAXCONN_PATH).unwrap_or(DEFAULT);

    match chosen.try_into() {
        Ok(val) => val,
        Err(_) => {
            warn!(
                "Backlog value out of i32 range (clamping to i32::MAX): {}",
                chosen
            );
            i32::MAX
        }
    }
}

/// Returns the optional TCP receive buffer size (`SO_RCVBUF`) to apply.
pub(super) fn tcp_recvbuf_size() -> Option<usize> {
    env_or_sys("TCP_RCVBUF", "/proc/sys/net/core/rmem_default")
}

/// Returns the optional TCP send buffer size (`SO_SNDBUF`) to apply.
pub(super) fn tcp_sendbuf_size() -> Option<usize> {
    env_or_sys("TCP_SNDBUF", "/proc/sys/net/core/wmem_default")
}

/// Returns the optional UDP receive buffer size (`SO_RCVBUF`) to apply.
pub(super) fn udp_recvbuf_size() -> Option<usize> {
    env_or_sys("UDP_RCVBUF", "/proc/sys/net/core/rmem_default")
}

/// Reads a value from either an environment variable or a sysctl file.
fn env_or_sys(env_key: &str, sys_path: &str) -> Option<usize> {
    std::env::var(env_key)
        .ok()
        .and_then(|v| v.parse().ok())
        .or_else(|| read_sys_default(sys_path))
}

/// Reads a kernel default value from the given sysctl file, trimming whitespace and parsing it as `usize`.
fn read_sys_default(path: &str) -> Option<usize> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
}
