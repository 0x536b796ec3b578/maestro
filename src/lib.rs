#![forbid(unsafe_code)]
//! # Maestro
//! A unified framework for creating and managing network services.
//!
//! This library’s purpose is to simplify the creation, supervision, and coordination of network-based services.
//! It provides a structured, asynchronous foundation for managing TCP and UDP protocols, handling service lifecycles, and ensuring robust, scalable operation.
//! By abstracting complex networking patterns, it allows developers to focus on service logic while the library handles concurrency, fault tolerance, and graceful shutdown.
//!
//! # Example
//! Simple `TCP` & `UDP` services:
//!
//! ```rust,no_run
//! pub struct MyTcpService;
//!
//! #[async_trait]
//! impl TcpHandler for MyTcpService {
//!     fn name(&self) -> &'static str {
//!         "MyTcpService"
//!     }
//!
//!     fn port(&self) -> u16 {
//!         8080
//!     }
//!
//!     async fn on_connection(&self, stream: TcpStream, peer: &SocketAddr, network_interface: &NetworkInterface) {
//!         unimplemented!()
//!     }
//! }
//!
//! pub struct MyUdpService;
//!
//! #[async_trait]
//! impl UdpRuntime for MyUdpService {
//!     fn name(&self) -> &'static str {
//!         "MyUdpService"
//!     }
//!
//!     fn port(&self) -> u16 {
//!         5353
//!     }
//!
//!     async fn on_packet(&self, data: &[u8], network_interface: &NetworkInterface, socket: Arc<UdpSocket>, peer: &SocketAddr) {
//!         unimplemented!()
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let network_interface = NetworkInterface::from_str("eth0")?;
//!     let mut supervisor = Supervisor::new(network_interface);
//!     supervisor.add(MyUdpService);
//!     supervisor.add(MyTcpService);
//!     supervisor.run().await?
//! }
//! ```
mod network;
mod runtime;
mod supervisor;
mod worker;

pub use network::{interface::NetworkInterface, socket::bind::BindMode};
pub use runtime::{tcp::TcpHandler, udp::UdpHandler};
pub use supervisor::{Supervisor, policy::RestartPolicy};
