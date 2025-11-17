# Maestro
*A lightweight, fast, and ergonomic framework for building TCP & UDP servers in Rust with zero boilerplate.*

## Overview

Maestro is a networking library that enables you to build high-performance concurrent TCP and UDP servers in Rust. By implementing just two lightweight traits, you can focus on your application's core logic while Maestro handles the network orchestration:
- `TcpHandler`
- `UdpHandler`

In other words:
> You compose the logic. Maestro conducts the orchestra.

## How It Works

### Implement a Handler

#### TCP Handler
```rust
struct MyTcpService;

#[async_trait]
impl TcpHandler for MyTcpService {
    fn name(&self) -> &'static str {
        "My TCP Service"
    }
    
    fn port(&self) -> u16 {
        8080
    }

    async fn on_connection(&self, stream: TcpStream, peer: &SocketAddr) {
        unimplemented!()
    }
}
```

#### UDP Handler
```rust
struct MyUdpService;

#[async_trait]
impl UdpHandler for MyUdpService {
    fn name(&self) -> &'static str {
        "My UDP Service"
    }
    
    fn port(&self) -> u16 {
        5353
    }

    async fn on_packet(&self, data: &[u8], socket: Arc<UdpSocket>, peer: &SocketAddr) {
        unimplemented!()
    }
}
```

### Registering Services with the `Supervisor`
```rust
#[tokio::main]
async fn main() -> Result<()> {
    let network_interface = NetworkInterface::from_str("lo")?;
    let mut supervisor = Supervisor::new(network_interface);
    
    supervisor.add(MyUdpService);
    supervisor.add(MyTcpService);
    
    supervisor.run().await?;
    
    Ok(())
}
```

## Installation

Run the following Cargo command in your project directory:
```bash
cargo add maestro
```

Or add the following line to your `Cargo.toml`:
```toml
maestro = "0.1.0"
```

## Contributing
We welcome contributions! Here are some good areas to get involved:
- Optimizing network performance and efficiency
- Writing benchmark tests to track and validate performance improvements
- Enhancing documentation for clarity and completeness
- Ensuring cross-platform compatibility

Before submitting a PR, please ensure that your changes are properly formatted by running:
```bash
cargo fmt
cargo clippy
```

## Supporting

Author: *Skynõx*

If you'd like to support the project, you can donate via the following addresses:
| Bitcoin  | bc1q87r2z8szxwqt538edzw5gl397c9v3hzxwjw82h |
| :------- | :----------------------------------------- |
| Ethereum | 0xe277049067F72E89326c2C0D11333531d5BbB78B |

---

> `Maestro` - one who conducts. Just like its name, this library orchestrates the networking layer so your application can focus on the melody of your logic.
