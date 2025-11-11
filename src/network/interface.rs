use getifaddrs::{Address, getifaddrs, if_nametoindex};
use std::{
    io::{Error, ErrorKind},
    net::{Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

/// Represents a local network interface with its associated metadata.
///
/// The interface includes its system name, index, IP addresses, and
/// optionally a MAC address. If no hardware address is found, a
/// locally-administered MAC is generated to maintain identity consistency.
///
/// # Example
/// ```rust,no_run
/// use maestro::NetworkInterface;
///
/// let iface = NetworkInterface::from_str("eth0")
///     .expect("Interface not found");
///
/// println!("Interface {} has IPv4 {:?}", iface.name, iface.inet);
/// ```
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    /// System interface name (e.g., `"eth0"` or `"lo"`).
    pub name: String,
    /// Operating system interface index.
    pub index: u32,
    /// List of assigned IPv4 addresses.
    pub inet: Vec<Ipv4Addr>,
    /// List of assigned IPv6 addresses.
    pub inet6: Vec<Ipv6Addr>,
    /// Optional hardware (MAC) address.
    pub mac: Option<[u8; 6]>,
}

impl NetworkInterface {
    /// Generates a pseudo-random, locally-administered MAC address.
    ///
    /// The address conforms to standard conventions by ensuring the
    /// "locally administered" and "unicast" bits are set appropriately.
    fn generate_mac(&self) -> [u8; 6] {
        let mut mac = [0u8; 6];
        rand::fill(&mut mac);
        mac[0] = (mac[0] & 0b11111110) | 0b00000010; // Set the local/unicast bit to remain realistic
        mac
    }

    /// Assigns a specific MAC address to this interface.
    fn _set_mac(&mut self, mac: [u8; 6]) {
        self.mac = Some(mac)
    }
}

/// Builds a [`NetworkInterface`] from its system name.
///
/// The function:
/// - Validates the provided interface name.
/// - Uses [`getifaddrs()`] to gather address information.
/// - Retrieves the interface index via [`if_nametoindex()`].
/// - Generates a pseudo-MAC if none exists.
///
/// # CLI Integration
/// This implementation was designed to integrate directly with
/// [`clap`](https://docs.rs/clap) argument parsing.
/// By implementing [`FromStr`], [`NetworkInterface`] can be parsed
/// automatically from a string argument using `value_parser!()`.
///
/// ```rust,no_run
/// use clap::{Parser, value_parser};
///
/// use maestro::NetworkInterface;
///
/// #[derive(Parser, Debug)]
/// struct Parameters {
///     /// Network interface to bind for poisoning and serving.
///     #[arg(short = 'i', long = "interface", value_name = "IFACE", value_parser = value_parser!(NetworkInterface))]
///     network_interface: NetworkInterface,
/// }
///
/// pub fn parse() -> Parameters {
///     Parameters::parse()
/// }
///
/// fn main() {
///     let params = parse();
///     let iface = params.network_interface;
///     println!("Selected interface: {}", iface.name);
/// }
/// ```
///
/// This makes it possible to write intuitive CLI tools that accept
/// interface names directly, without manual lookups or conversions.
///
/// # Example
/// ```rust,no_run
/// use std::str::FromStr;
/// use maestro::NetworkInterface;
///
/// let iface = NetworkInterface::from_str("lo").unwrap();
/// println!("IPv4: {:?}", iface.net);
/// ```
impl FromStr for NetworkInterface {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if name.trim().is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Interface name is empty.",
            ));
        }

        let index = if_nametoindex(name)?;
        let mut information = Self {
            name: name.to_string(),
            index,
            inet: Vec::new(),
            inet6: Vec::new(),
            mac: None,
        };

        for iface in getifaddrs()? {
            if iface.name == name {
                match iface.address {
                    Address::V4(v4) => information.inet.push(v4.address),
                    Address::V6(v6) => information.inet6.push(v6.address),
                    Address::Mac(mac) => information.mac = Some(mac),
                }
            }
        }

        if information.inet.is_empty() && information.inet6.is_empty() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("Interface `{name}` not found."),
            ));
        }

        if information.mac.is_none() {
            information.mac = Some(information.generate_mac())
        }

        Ok(information)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn empty_name() {
        let result = NetworkInterface::from_str("");
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::InvalidInput));
    }

    #[test]
    fn existing_interface() {
        let info = NetworkInterface::from_str("lo").expect("Failed to get interface info");
        assert_eq!(info.name, "lo");
        assert!(
            !info.inet.is_empty() || !info.inet6.is_empty(),
            "Interface should have at least one IP"
        );
        if !info.inet.is_empty() {
            assert!(
                info.inet.contains(&Ipv4Addr::LOCALHOST),
                "IPv4 localhost missing"
            );
        }
        if !info.inet6.is_empty() {
            assert!(
                info.inet6.contains(&Ipv6Addr::LOCALHOST),
                "IPv6 localhost missing"
            );
        }
    }

    #[test]
    fn non_existing_interface() {
        let result = NetworkInterface::from_str("fake0");
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::NotFound));
    }

    #[test]
    fn mac_is_generated_if_none() {
        let mut info = NetworkInterface {
            name: "test0".to_string(),
            index: 0,
            inet: vec![Ipv4Addr::LOCALHOST],
            inet6: vec![Ipv6Addr::LOCALHOST],
            mac: None,
        };
        info.mac = Some(info.generate_mac());
        let mac = info.mac.unwrap();
        assert_eq!(mac.len(), 6);
        assert_eq!(mac[0] & 0b00000011, 0b00000010);
    }

    #[test]
    fn mac_is_preserved_if_present() {
        let existing_mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let info = NetworkInterface {
            name: "test1".to_string(),
            index: 1,
            inet: vec![Ipv4Addr::LOCALHOST],
            inet6: vec![Ipv6Addr::LOCALHOST],
            mac: Some(existing_mac),
        };
        let mac = info.mac.unwrap();
        assert_eq!(mac, existing_mac);
    }
}
