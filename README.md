# Maestro
*A lightweight, fast, and ergonomic framework for building TCP & UDP servers in Rust with zero boilerplate.*

## Overview

Maestro is a networking library that lets you build concurrent TCP and UDP servers by implementing just two lightweight traits:
- `TcpHandler`
- `UdpHandler`

In other words:
> You write the logic. Maestro conducts the orchestra.

## How It Works

### Implement a handler

#### TCP
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

    async fn on_connection(
        &self,
        stream: TcpStream,
        peer: &SocketAddr,
        network_interface: &NetworkInterface,
    ) {
        unimplemented!()
    }
}
```

#### UDP
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

### Register services into the `Supervisor`
```rust
#[tokio::main]
async fn main() -> Result<()> {
    let network_interface = NetworkInterface::from_str("eth0")?;
    let mut supervisor = Supervisor::new(network_interface);
    supervisor.add(MyUdpService);
    supervisor.add(MyTcpService);
    supervisor.run().await?;
    Ok(())
}
```

## Installation

### Cargo:
```
[dependencies]
maestro = "0.1.0"
```

### Or install from source:
```
git clone https://github.com/0x536b796ec3b578/maestro
cd maestro
cargo build --release
```

## Contributing
Contributions are always welcome!

Good areas to contribute:
- Performance improvements (avoiding allocations, reducing syscalls)
- New socket options or advanced tuning knobs
- More automatic interface detection
- Additional helper utilities for common protocols
- Improved diagnostics & tracing integration
- Cross-platform support for macOS/Windows/BSD

Before submitting a PR, please run cargo fmt and cargo clippy to maintain consistent formatting and lint standards.

## Supporting

Author: Skynõx

If you’d like to support the project:
| Bitcoin  | bc1q87r2z8szxwqt538edzw5gl397c9v3hzxwjw82h |
| :------- | :----------------------------------------- |
| Ethereum | 0xe277049067F72E89326c2C0D11333531d5BbB78B |

---

> `Maestro` - one who conducts. Just like its name, this library orchestrates the networking layer so your application can focus on the melody of your logic.
