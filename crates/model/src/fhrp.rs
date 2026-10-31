//! First-hop redundancy beyond HSRP: VRRP (RFC 5798; version 2 per RFC 3768 by default) and
//! GLBP groups, kept per interface name (`GigabitEthernet0/0`, `GigabitEthernet0/0.10`,
//! `Vlan10`), so they need no field on every kind of interface.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

/// Most groups of one protocol on one interface, and interfaces with groups.
pub const MAX_GROUPS: usize = 16;
pub const MAX_IFACES: usize = 256;

/// `vrrp N …` on an interface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Vrrp {
    /// The virtual router id, 1-255.
    pub group: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<Ipv4Addr>,
    /// 1-254, 100 by default (255 is the address owner's, taken automatically).
    pub priority: u8,
    /// On by default (RFC 5798 §6.1); `no vrrp N preempt` turns it off.
    pub preempt: bool,
    /// `vrrp N timers advertise S` (`… msec M` with version 3): the advertisement interval in
    /// centiseconds (100, one second, when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advertise_cs: Option<u16>,
    /// `vrrp N track INTERFACE [decrement D]`: while that interface is down, the priority drops
    /// by D (10 by default).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub track: Vec<(String, u8)>,
    /// `vrrp N ipv6 FE80::…`: the group's IPv6 virtual link-local address (VRRP version 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<Ipv6Addr>,
}

/// Most tracked interfaces per VRRP group.
pub const MAX_TRACK: usize = 8;
/// Longest advertisement interval (RFC 5798's 12-bit field, centiseconds).
pub const MAX_ADVERTISE_CS: u16 = 4095;

impl Vrrp {
    pub fn new(group: u8) -> Vrrp {
        Vrrp { group, ip: None, priority: 100, preempt: true, advertise_cs: None, track: Vec::new(), ipv6: None }
    }

    /// The advertisement interval, centiseconds.
    pub fn interval_cs(&self) -> u16 {
        self.advertise_cs.unwrap_or(100).clamp(1, MAX_ADVERTISE_CS)
    }
}

/// How a GLBP gateway shares hosts among the forwarders.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GlbpBalance {
    /// Each ARP request gets the next forwarder.
    #[default]
    RoundRobin,
    /// A host always gets the same forwarder (by its MAC address).
    HostDependent,
}

impl GlbpBalance {
    pub fn name(self) -> &'static str {
        match self {
            GlbpBalance::RoundRobin => "round-robin",
            GlbpBalance::HostDependent => "host-dependent",
        }
    }

    pub fn parse(s: &str) -> Option<GlbpBalance> {
        match s.trim().to_ascii_lowercase().as_str() {
            "round-robin" => Some(GlbpBalance::RoundRobin),
            "host-dependent" => Some(GlbpBalance::HostDependent),
            _ => None,
        }
    }
}

/// `glbp N …` on an interface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Glbp {
    /// 0-1023.
    pub group: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<Ipv4Addr>,
    /// 1-255, 100 by default: the highest is the active virtual gateway.
    pub priority: u8,
    /// Off by default.
    #[serde(default)]
    pub preempt: bool,
    #[serde(default)]
    pub balance: GlbpBalance,
}

impl Glbp {
    pub fn new(group: u16) -> Glbp {
        Glbp { group, ip: None, priority: 100, preempt: false, balance: GlbpBalance::default() }
    }
}

/// A device's VRRP and GLBP groups, by interface name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Fhrp {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub vrrp: BTreeMap<String, Vec<Vrrp>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub glbp: BTreeMap<String, Vec<Glbp>>,
    /// `fhrp version vrrp v3`: advertisements in the RFC 5798 format (else RFC 3768's).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub vrrp_v3: bool,
}

impl Fhrp {
    pub fn is_empty(&self) -> bool {
        *self == Fhrp::default()
    }

    pub fn vrrp_on(&self, iface: &str) -> &[Vrrp] {
        self.vrrp.get(iface).map_or(&[], Vec::as_slice)
    }

    pub fn glbp_on(&self, iface: &str) -> &[Glbp] {
        self.glbp.get(iface).map_or(&[], Vec::as_slice)
    }

    /// Caps every list (hostile files) and drops empty entries.
    pub fn sanitize(&mut self) {
        self.vrrp.retain(|_, v| !v.is_empty());
        self.glbp.retain(|_, v| !v.is_empty());
        while self.vrrp.len() > MAX_IFACES {
            self.vrrp.pop_last();
        }
        while self.glbp.len() > MAX_IFACES {
            self.glbp.pop_last();
        }
        for v in self.vrrp.values_mut() {
            v.truncate(MAX_GROUPS);
            for g in v.iter_mut() {
                g.group = g.group.max(1);
                g.priority = g.priority.clamp(1, 254);
                g.track.truncate(MAX_TRACK);
                g.advertise_cs = g.advertise_cs.map(|a| a.clamp(1, MAX_ADVERTISE_CS));
            }
        }
        for v in self.glbp.values_mut() {
            v.truncate(MAX_GROUPS);
            for g in v.iter_mut() {
                g.group = g.group.min(1023);
                g.priority = g.priority.max(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_sanitize() {
        assert!(Vrrp::new(1).preempt && Vrrp::new(1).priority == 100);
        assert!(!Glbp::new(1).preempt);
        assert_eq!(GlbpBalance::parse("Host-Dependent"), Some(GlbpBalance::HostDependent));
        let mut f = Fhrp::default();
        assert!(f.is_empty());
        f.vrrp.insert(
            "g0/0".into(),
            vec![Vrrp { group: 0, priority: 255, advertise_cs: Some(9_999), track: vec![(String::new(), 1); 20], ..Vrrp::new(1) }; 40],
        );
        f.glbp.insert("g0/1".into(), vec![Glbp { group: 5000, ..Glbp::new(1) }]);
        f.glbp.insert("g0/2".into(), Vec::new());
        f.sanitize();
        let v = f.vrrp_on("g0/0");
        assert_eq!(v.len(), MAX_GROUPS);
        assert!(v.iter().all(|g| g.group == 1 && g.priority == 254 && g.track.len() == MAX_TRACK && g.interval_cs() == MAX_ADVERTISE_CS));
        assert_eq!(Vrrp::new(1).interval_cs(), 100);
        assert_eq!(f.glbp_on("g0/1").first().map(|g| g.group), Some(1023));
        assert!(f.glbp_on("g0/2").is_empty() && !f.glbp.contains_key("g0/2"));
    }
}
