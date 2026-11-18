//! VRF-lite: virtual routing and forwarding instances on one router (`ip vrf NAME`).
//!
//! A VRF has its own routing table and its own ARP table. The interfaces that belong to it
//! (`ip vrf forwarding NAME`) are not in the global table, and a packet that comes in on one of
//! them is routed only with what the VRF knows, so two VRFs of one router can use the same
//! addresses and never reach each other. Each VRF carries its own static routes, an OSPF process
//! (`router ospf N vrf NAME`) and a BGP address family (`address-family ipv4 vrf NAME`).
//!
//! The route distinguisher and the route targets are kept and shown (they are what a provider
//! edge exports and imports); VRF-lite itself does not use them to move routes between VRFs.

use serde::{Deserialize, Serialize};

use crate::bgp::Bgp;
use crate::{Device, Ospf, StaticRoute};

/// Most VRFs on one device.
pub const MAX_VRFS: usize = 16;
/// Longest VRF name.
pub const MAX_NAME: usize = 32;
/// Most route targets one VRF imports or exports.
pub const MAX_TARGETS: usize = 16;

/// One VRF of a router.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vrf {
    pub name: String,
    /// `rd ASN:NN` or `rd A.B.C.D:NN`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rd: Option<String>,
    /// `route-target import ASN:NN`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub import: Vec<String>,
    /// `route-target export ASN:NN`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub export: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The interfaces in the VRF, by name (`GigabitEthernet0/0`, `GigabitEthernet0/0.10`,
    /// `Vlan10`, `Loopback1`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    /// `ip route vrf NAME ...`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<StaticRoute>,
    /// `router ospf N vrf NAME`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ospf: Option<Ospf>,
    /// `address-family ipv4 vrf NAME` under `router bgp N`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bgp: Option<Bgp>,
}

impl Vrf {
    pub fn new(name: &str) -> Vrf {
        Vrf { name: name.to_string(), ..Vrf::default() }
    }
}

impl Device {
    /// The VRF with this name (names are case-sensitive).
    pub fn vrf(&self, name: &str) -> Option<&Vrf> {
        self.vrfs.iter().find(|v| v.name == name)
    }

    /// The VRF an interface belongs to, by its name; `None` for the global table.
    pub fn vrf_of(&self, iface: &str) -> Option<&str> {
        self.vrfs.iter().find(|v| v.members.iter().any(|m| m == iface)).map(|v| v.name.as_str())
    }

    /// Puts an interface in a VRF (`Some`) or back in the global table (`None`); the VRF must
    /// exist. An interface is in at most one VRF.
    pub fn set_vrf_of(&mut self, iface: &str, vrf: Option<&str>) -> bool {
        if let Some(name) = vrf
            && self.vrf(name).is_none()
        {
            return false;
        }
        for v in &mut self.vrfs {
            v.members.retain(|m| m != iface);
        }
        if let Some(name) = vrf
            && let Some(v) = self.vrfs.iter_mut().find(|v| v.name == name)
        {
            v.members.push(iface.to_string());
            v.members.sort();
        }
        true
    }
}

/// Whether `text` is a route distinguisher or route target: `ASN:NN` or `A.B.C.D:NN`.
pub fn valid_rd(text: &str) -> bool {
    let Some((left, right)) = text.split_once(':') else { return false };
    let number = |s: &str| !s.is_empty() && s.len() <= 10 && s.chars().all(|c| c.is_ascii_digit()) && s.parse::<u64>().is_ok();
    (number(left) || left.parse::<std::net::Ipv4Addr>().is_ok()) && number(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rd_forms() {
        assert!(valid_rd("65000:1"));
        assert!(valid_rd("10.0.0.1:20"));
        assert!(!valid_rd("65000"));
        assert!(!valid_rd(":1"));
        assert!(!valid_rd("a:b"));
    }
}
