//! IGMP settings: a switch's snooping and querier, and the groups a host has joined.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// Most groups a host joins.
pub const MAX_JOINED: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Igmp {
    /// Switches: `ip igmp snooping`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub on: bool,
    /// Switches: VLANs with `no ip igmp snooping vlan N`.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub vlan_off: BTreeSet<u16>,
    /// Switches: `ip igmp snooping querier`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub querier: bool,
    /// Switches: `ip igmp snooping querier address A`, the source of the queries when a VLAN has
    /// no interface address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub querier_address: Option<Ipv4Addr>,
    /// Hosts: the groups they have joined (`igmp join`).
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub joined: BTreeSet<Ipv4Addr>,
}

impl Igmp {
    /// Whether the switch snoops a VLAN.
    pub fn active(&self, vlan: u16) -> bool {
        self.on && !self.vlan_off.contains(&vlan)
    }

    /// Whether an address is a group a host may join: a multicast address outside 224.0.0.0/24,
    /// which belongs to protocols on the link.
    pub fn joinable(group: Ipv4Addr) -> bool {
        group.is_multicast() && group.octets().get(..3) != Some(&[224, 0, 0][..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_groups_can_be_joined() {
        assert!(Igmp::joinable(Ipv4Addr::new(239, 1, 1, 1)));
        assert!(Igmp::joinable(Ipv4Addr::new(224, 0, 1, 1)));
        assert!(!Igmp::joinable(Ipv4Addr::new(224, 0, 0, 5)));
        assert!(!Igmp::joinable(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn snooping_is_per_vlan_once_it_is_on() {
        let mut i = Igmp::default();
        assert!(!i.active(10));
        i.on = true;
        i.vlan_off.insert(20);
        assert!(i.active(10) && !i.active(20));
    }
}
