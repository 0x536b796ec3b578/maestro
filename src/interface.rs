use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

use getifaddrs::{Address, InterfaceFlags, getifaddrs, if_nametoindex};

use crate::{Error, Result};

/// A local network interface and the addresses assigned to it.
///
/// This is a snapshot taken when the value is created. It does not track
/// addresses added or removed afterwards.
///
/// # Examples
///
/// ```no_run
/// use maestro_rs::NetworkInterface;
///
/// let eth0: NetworkInterface = "eth0".parse()?;
/// println!("{eth0}: {:?} {:?}", eth0.ipv4(), eth0.ipv6());
///
/// // Portable way to get the loopback interface:
/// let lo = NetworkInterface::loopback()?;
/// # Ok::<(), maestro_rs::Error>(())
/// ```
///
/// Because it implements [`FromStr`], it can be used directly as a
/// [`clap`](https://docs.rs/clap) argument with `value_parser!(NetworkInterface)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInterface {
    name: String,
    index: u32,
    ipv4: Vec<Ipv4Addr>,
    ipv6: Vec<Ipv6Addr>,
    mac: Option<[u8; 6]>,
    loopback: bool,
    alias: Option<String>,
}

impl NetworkInterface {
    /// Looks up an interface by its system name (e.g. `"eth0"`, `"lo0"`).
    ///
    /// On Windows the friendly name shown in the control panel (e.g.
    /// `"Ethernet"`, matched case-insensitively) is accepted as well as the
    /// system name (e.g. `"ethernet_32768"`). See [`alias`](Self::alias).
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidInterfaceName`] if `name` is empty or contains a NUL byte.
    /// - [`Error::InterfaceNotFound`] if no such interface exists.
    /// - [`Error::Io`] if the system interface list cannot be read.
    pub fn from_name(name: &str) -> Result<Self> {
        if name.trim().is_empty() || name.contains('\0') {
            return Err(Error::InvalidInterfaceName(name.to_owned()));
        }

        Self::list()?
            .into_iter()
            .find(|iface| {
                iface.name == name
                    || iface
                        .alias
                        .as_deref()
                        .is_some_and(|alias| alias.eq_ignore_ascii_case(name))
            })
            .ok_or_else(|| Error::InterfaceNotFound(name.to_owned()))
    }

    /// The first loopback interface: `lo` on Linux, `lo0` on macOS and BSD,
    /// `loopback_0` on Windows.
    ///
    /// # Errors
    ///
    /// - [`Error::InterfaceNotFound`] if the system has no loopback interface.
    /// - [`Error::Io`] if the system interface list cannot be read.
    pub fn loopback() -> Result<Self> {
        Self::list()?
            .into_iter()
            .find(Self::is_loopback)
            .ok_or_else(|| Error::InterfaceNotFound("loopback".to_owned()))
    }

    /// Lists every network interface on the system, in the order the OS reports them.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the system interface list cannot be read.
    pub fn list() -> Result<Vec<Self>> {
        let mut out: Vec<Self> = Vec::new();
        for entry in getifaddrs()? {
            let pos = match out.iter().position(|i| i.name == entry.name) {
                Some(pos) => pos,
                None => {
                    let index = match entry.index {
                        Some(index) => index,
                        None => if_nametoindex(&entry.name)?,
                    };
                    #[cfg_attr(not(windows), expect(unused_mut))]
                    let mut iface = Self::empty(entry.name, index);
                    #[cfg(windows)]
                    {
                        iface.alias = Some(entry.description).filter(|d| !d.is_empty());
                    }
                    out.push(iface);
                    out.len() - 1
                }
            };
            out[pos].absorb(entry.address, entry.flags);
        }
        Ok(out)
    }

    /// The system name, e.g. `"eth0"`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The OS interface index (used as the IPv6 scope ID).
    #[must_use]
    pub fn index(&self) -> u32 {
        self.index
    }

    /// IPv4 addresses assigned to the interface.
    #[must_use]
    pub fn ipv4(&self) -> &[Ipv4Addr] {
        &self.ipv4
    }

    /// IPv6 addresses assigned to the interface.
    #[must_use]
    pub fn ipv6(&self) -> &[Ipv6Addr] {
        &self.ipv6
    }

    /// All IPv4 then IPv6 addresses assigned to the interface.
    pub fn addrs(&self) -> impl Iterator<Item = IpAddr> + '_ {
        let v4 = self.ipv4.iter().copied().map(IpAddr::V4);
        let v6 = self.ipv6.iter().copied().map(IpAddr::V6);
        v4.chain(v6)
    }

    /// The hardware (MAC) address, if the interface has one.
    ///
    /// Loopback and tunnel interfaces typically have none.
    #[must_use]
    pub fn mac(&self) -> Option<[u8; 6]> {
        self.mac
    }

    /// The Windows friendly name (e.g. `"Ethernet"`). Always `None` on other platforms.
    #[must_use]
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    /// Whether this is a loopback interface.
    #[must_use]
    pub fn is_loopback(&self) -> bool {
        self.loopback
    }

    fn empty(name: String, index: u32) -> Self {
        Self {
            name,
            index,
            ipv4: Vec::new(),
            ipv6: Vec::new(),
            mac: None,
            loopback: false,
            alias: None,
        }
    }

    fn absorb(&mut self, address: Address, flags: InterfaceFlags) {
        self.loopback |= flags.contains(InterfaceFlags::LOOPBACK);
        match address {
            Address::V4(v4) => self.ipv4.push(v4.address),
            Address::V6(v6) => self.ipv6.push(v6.address),
            // Some platforms report an all-zero MAC for interfaces without one.
            Address::Mac(mac) if mac != [0; 6] => self.mac = Some(mac),
            Address::Mac(_) => {}
        }
    }
}

impl FromStr for NetworkInterface {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self> {
        Self::from_name(name)
    }
}

impl fmt::Display for NetworkInterface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.name)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn loopback() -> NetworkInterface {
        NetworkInterface::loopback().expect("system has a loopback interface")
    }

    #[test]
    fn list_contains_loopback_with_localhost() {
        let lo = loopback();
        assert!(lo.ipv4().contains(&Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn from_name_matches_list_entry() {
        let lo = loopback();
        let by_name = NetworkInterface::from_name(lo.name()).unwrap();
        assert_eq!(by_name, lo);
    }

    #[test]
    fn empty_name_is_invalid() {
        assert!(matches!(
            "  ".parse::<NetworkInterface>(),
            Err(Error::InvalidInterfaceName(_))
        ));
    }

    #[test]
    fn numeric_name_is_not_treated_as_index() {
        assert!(matches!(
            NetworkInterface::from_name("4242"),
            Err(Error::InterfaceNotFound(_))
        ));
    }

    #[test]
    fn unknown_name_is_not_found() {
        assert!(matches!(
            NetworkInterface::from_name("maestro-nope0"),
            Err(Error::InterfaceNotFound(_))
        ));
    }
}
