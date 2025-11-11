#![forbid(unsafe_code)]

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
//! use async_trait::async_trait;
//! use std::io::Result;
//!
//! use maestro::{NetworkInterface, RestartPolicy, Supervisor, TcpService, UdpService};
//!
//! pub struct MyTcpService;
//!
//! #[async_trait]
//! impl TcpService for MyTcpService {
//!     fn name(&self) -> &'static str {
//!         "MyTcpService"
//!     }
//!
//!     fn port(&self) -> u16 {
//!         8080
//!     }
//!
//!     async fn on_connection(&self, network_interface: &NetworkInterface, peer: &SocketAddr, mut stream: TcpStream) {
//!         unimplemented!()
//!     }
//! }
//!
//! pub struct MyUdpService;
//!
//! #[async_trait]
//! impl UdpService for MyUdpService {
//!     fn name(&self) -> &'static str {
//!         "MyUdpService"
//!     }
//!
//!     fn port(&self) -> u16 {
//!         5353
//!     }
//!
//!     async fn on_packet(&self, data: &[u8], network_interface: &NetworkInterface, peer: &SocketAddr, socket: Arc<UdpSocket>) {
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
mod service;
mod supervisor;
mod worker;

pub use network::{
    interface::NetworkInterface,
    protocol::{Tcp, TcpHandler as TcpService, Udp, UdpHandler as UdpService},
};
pub use service::BindMode;
pub use supervisor::{RestartPolicy, Supervisor};

pub(crate) use service::{Service, ServiceAdapter};
pub(crate) use worker::{Worker, WorkerService};
