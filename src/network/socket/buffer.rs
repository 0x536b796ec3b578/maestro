use std::fs;

use tracing::warn;

pub(super) fn tcp_backlog() -> i32 {
    const DEFAULT: usize = 128;
    #[cfg(target_os = "linux")]
    const SOMAXCONN_PATH: &str = "/proc/sys/net/core/somaxconn";
    #[cfg(not(target_os = "linux"))]
    const SOMAXCONN_PATH: &str = ""; // dummy value, won't be read

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

pub(super) fn tcp_recvbuf_size() -> Option<usize> {
    env_or_sys("TCP_RCVBUF", "/proc/sys/net/core/rmem_default")
}

pub(super) fn tcp_sendbuf_size() -> Option<usize> {
    env_or_sys("TCP_SNDBUF", "/proc/sys/net/core/wmem_default")
}

pub(super) fn udp_recvbuf_size() -> Option<usize> {
    env_or_sys("UDP_RCVBUF", "/proc/sys/net/core/rmem_default")
}

fn env_or_sys(env_key: &str, sys_path: &str) -> Option<usize> {
    std::env::var(env_key)
        .ok()
        .and_then(|v| v.parse().ok())
        .or_else(|| read_sys_default(sys_path))
}

fn read_sys_default(path: &str) -> Option<usize> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
}
