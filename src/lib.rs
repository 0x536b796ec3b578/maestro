//! # maestro-rs
//!
//! Supervised TCP and UDP services bound to a specific network interface.
//!
//! Implement [`TcpHandler`] or [`UdpHandler`] for your protocol logic, register
//! the handlers on a [`Supervisor`] for a [`NetworkInterface`], and call
//! [`Supervisor::run`]. Maestro takes care of:
//!
//! - **Binding**: to every address of the interface (including scoped IPv6
//!   link-local addresses), to all interfaces, or to one address. See [`BindMode`].
//! - **Concurrency**: one task per TCP connection, and UDP receive work on
//!   every CPU core (`SO_REUSEPORT` sharding on Linux, shared sockets elsewhere).
//! - **Portability**: Linux, macOS, Windows and BSD. See
//!   [`NetworkInterface::loopback`] for a portable loopback interface.
//! - **Multicast**: joining groups on the chosen interface.
//!   See [`UdpHandler::multicast_groups`].
//! - **Supervision**: restarting failed services with exponential backoff.
//!   See [`RestartPolicy`].
//! - **Shutdown**: on Ctrl+C, SIGTERM or Windows console close, or on any future you pass to
//!   [`Supervisor::run_until`], services stop and in-flight work gets time to finish.
//!
//! # Example
//!
//! ```no_run
//! use maestro_rs::{NetworkInterface, Result, Supervisor, TcpHandler, UdpHandler};
//! use std::{net::SocketAddr, sync::Arc};
//! use tokio::{
//!     io::{AsyncReadExt, AsyncWriteExt},
//!     net::{TcpStream, UdpSocket},
//! };
//!
//! struct Echo;
//!
//! impl TcpHandler for Echo {
//!     fn name(&self) -> &str { "tcp-echo" }
//!     fn port(&self) -> u16 { 8080 }
//!
//!     async fn on_connection(&self, mut stream: TcpStream, _peer: SocketAddr) {
//!         let mut buf = [0u8; 1024];
//!         while let Ok(n @ 1..) = stream.read(&mut buf).await {
//!             if stream.write_all(&buf[..n]).await.is_err() {
//!                 break;
//!             }
//!         }
//!     }
//! }
//!
//! impl UdpHandler for Echo {
//!     fn name(&self) -> &str { "udp-echo" }
//!     fn port(&self) -> u16 { 8080 }
//!
//!     async fn on_packet(&self, data: &[u8], socket: &Arc<UdpSocket>, peer: SocketAddr) {
//!         let _ = socket.send_to(data, peer).await;
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let iface = NetworkInterface::loopback()?; // or "eth0".parse()?
//!     let mut supervisor = Supervisor::new(iface);
//!     // `Echo` implements both traits, so name the protocol explicitly.
//!     supervisor
//!         .add::<maestro_rs::Tcp, _>(Echo)
//!         .add::<maestro_rs::Udp, _>(Echo);
//!     supervisor.run().await
//! }
//! ```
//!
//! # Features
//!
//! - `tracing` (default): emit logs through [`tracing`](https://docs.rs/tracing).
//!   Install a subscriber in your application to see them.
#![cfg_attr(not(feature = "tracing"), allow(unused_variables))]

mod bind;
mod error;
mod interface;
mod log;
mod restart;
mod service;
mod supervisor;
mod tcp;
mod udp;

pub use bind::BindMode;
pub use error::{Error, Result};
pub use interface::NetworkInterface;
pub use restart::RestartPolicy;
pub use service::{Service, Tcp, Udp};
pub use supervisor::Supervisor;
pub use tcp::TcpHandler;
pub use udp::UdpHandler;
