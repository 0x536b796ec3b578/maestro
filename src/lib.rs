#![forbid(unsafe_code)]
//! # Maestro
//! `maestro` is a unified, high-performance framework for creating, supervising, and managing
//! asynchronous network services in Rust.
//!
//! This library’s purpose is to simplify the creation, supervision, and coordination of network-based services.
//! It provides a structured, asynchronous foundation for managing TCP and UDP protocols, handling service lifecycles, and ensuring robust, scalable operation.
//! By abstracting complex networking patterns, it allows developers to focus on service logic while the library handles concurrency, fault tolerance, and graceful shutdown.
//!
//! # Example
//!
//! ```rust,no_run
//! use maestro_rs::{NetworkInterface, Result, Supervisor, TcpHandler, UdpHandler, async_trait};
//! use std::{net::SocketAddr, str::FromStr, sync::Arc};
//! use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpStream, UdpSocket}};
//! use tracing::{error, info};
//!
//! struct MyTcpService;
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
//!     async fn on_connection(&self, mut stream: TcpStream, peer: &SocketAddr) {
//!         unimplemented!()
//!     }
//! }
//!
//! struct MyUdpService;
//!
//! #[async_trait]
//! impl UdpHandler for MyUdpService {
//!     fn name(&self) -> &'static str {
//!         "MyUdpService"
//!     }
//!
//!     fn port(&self) -> u16 {
//!         5353
//!     }
//!
//!     async fn on_packet(&self, data: &[u8], socket: Arc<UdpSocket>, peer: &SocketAddr) {
//!         unimplemented!()
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     tracing_subscriber::fmt()
//!         .with_max_level(tracing::Level::INFO)
//!         .init();
//!
//!     let iface = NetworkInterface::from_str("lo")?;
//!     let mut supervisor = Supervisor::new(iface);
//!
//!     supervisor.add(MyUdpService);
//!     supervisor.add(MyTcpService);
//!
//!     supervisor.run().await?;
//!
//!     Ok(())
//! }
//! ```
mod error;
mod handler;
mod network;
mod supervisor;

pub use async_trait::async_trait;
pub use error::{Error, Result};
pub use handler::{Tcp, TcpHandler, Udp, UdpHandler};
pub use network::{BindMode, NetworkInterface};
pub use supervisor::{RestartPolicy, Supervisor};
