//! Routing settings that are not a protocol process of their own: loopback interfaces, and
//! EIGRP summary addresses (kept with the interface, like the EIGRP timers).

use netlabcraft_core::{Ipv4Net, Ipv6Net};
use serde::{Deserialize, Serialize};

use crate::Ipv6Conf;

/// Most loopback interfaces per device.
pub const MAX_LOOPBACKS: usize = 256;
/// Highest loopback interface number.
pub const MAX_LOOPBACK_NUMBER: u32 = 2_147_483_647;

/// `interface Loopback N`: a virtual interface with no cable behind it. It is up whenever it is
/// not shut down, so its address stays reachable while any path to the router exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loopback {
    pub number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<Ipv4Net>,
    #[serde(default)]
    pub shutdown: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Ipv6Conf::is_default")]
    pub ipv6: Ipv6Conf,
}

impl Loopback {
    pub fn new(number: u32) -> Loopback {
        Loopback { number, ip: None, shutdown: false, description: String::new(), ipv6: Ipv6Conf::default() }
    }

    pub fn name(&self) -> String {
        format!("Loopback{}", self.number)
    }
}

/// The loopback number in an interface name (`Loopback0`, `loopback 0`, `lo0`), if it names one.
pub fn loopback_number(name: &str) -> Option<u32> {
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let split = compact.find(|c: char| c.is_ascii_digit())?;
    let (word, num) = compact.split_at(split);
    let word = word.to_ascii_lowercase();
    if word.len() < 2 || !"loopback".starts_with(&word) {
        return None;
    }
    num.parse::<u32>().ok().filter(|n| *n <= MAX_LOOPBACK_NUMBER)
}

impl crate::Device {
    /// The loopback interface, created (up, unconfigured) if missing. `None` past
    /// [`MAX_LOOPBACKS`].
    pub fn loopback_mut(&mut self, number: u32) -> Option<&mut Loopback> {
        if !self.loopbacks.iter().any(|l| l.number == number) {
            if self.loopbacks.len() >= MAX_LOOPBACKS {
                return None;
            }
            self.loopbacks.push(Loopback::new(number));
            self.loopbacks.sort_by_key(|l| l.number);
        }
        self.loopbacks.iter_mut().find(|l| l.number == number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_names() {
        assert_eq!(loopback_number("Loopback0"), Some(0));
        assert_eq!(loopback_number("loopback 12"), Some(12));
        assert_eq!(loopback_number("lo1"), Some(1));
        assert_eq!(loopback_number("l1"), None);
        assert_eq!(loopback_number("Lo99999999999"), None);
        assert_eq!(loopback_number("GigabitEthernet0/0"), None);
    }
}

/// Most summary addresses per interface.
pub const MAX_SUMMARIES: usize = 16;

/// `ip summary-address eigrp AS NET MASK` and `ipv6 summary-address eigrp AS PREFIX` on one
/// interface: (autonomous system, summary).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct EigrpSummaries {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub v4: Vec<(u16, Ipv4Net)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub v6: Vec<(u16, Ipv6Net)>,
}

impl EigrpSummaries {
    pub fn is_empty(&self) -> bool {
        self.v4.is_empty() && self.v6.is_empty()
    }

    /// The IPv4 summaries for one autonomous system.
    pub fn v4_for(&self, asn: u16) -> Vec<Ipv4Net> {
        self.v4.iter().filter(|(a, _)| *a == asn).map(|(_, n)| *n).collect()
    }

    /// The IPv6 summaries for one autonomous system.
    pub fn v6_for(&self, asn: u16) -> Vec<Ipv6Net> {
        self.v6.iter().filter(|(a, _)| *a == asn).map(|(_, n)| *n).collect()
    }
}

/// `ipv6 router rip NAME`: a router's RIPng process (RFC 2080). Interfaces join with
/// `ipv6 rip NAME enable`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rip6 {
    pub name: String,
    /// `timers basic UPDATE INVALID HOLDDOWN FLUSH` (seconds; 30 180 180 300 when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timers: Option<[u32; 4]>,
}

/// RIPng's timers when not set (RFC 2080 §2.3: update 30 s, timeout 180 s, garbage collection
/// 120 s after the timeout, so flush at 300 s from the last update; the hold-down as in RIP).
pub const RIP6_TIMERS: [u32; 4] = [30, 180, 180, 300];

impl Rip6 {
    /// Update, invalid, hold-down and flush times (seconds).
    pub fn timers(&self) -> (u32, u32, u32, u32) {
        let [u, i, h, f] = self.timers.unwrap_or(RIP6_TIMERS);
        (u.max(1), i.max(1), h, f.max(1))
    }
}

/// Longest RIPng process name.
pub const MAX_RIP6_NAME: usize = 32;
