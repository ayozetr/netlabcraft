//! Routing settings that are not a protocol process of their own: loopback interfaces.

use netlabcraft_core::Ipv4Net;
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
