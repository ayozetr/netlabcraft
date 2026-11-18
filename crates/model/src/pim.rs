//! Multicast routing settings: `ip multicast-routing`, PIM on each interface, the rendezvous
//! points and the groups a router's own interfaces have joined.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::acl::{Acl, PacketKey};

/// Most interfaces that hold PIM settings.
pub const MAX_IFACES: usize = 512;
/// Most static rendezvous point lines.
pub const MAX_RPS: usize = 32;
/// Most groups one interface joins.
pub const MAX_JOINED: usize = 32;

/// How PIM runs on an interface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PimMode {
    #[default]
    Sparse,
    Dense,
    SparseDense,
}

impl PimMode {
    /// The word that follows `ip pim`.
    pub fn word(self) -> &'static str {
        match self {
            PimMode::Sparse => "sparse-mode",
            PimMode::Dense => "dense-mode",
            PimMode::SparseDense => "sparse-dense-mode",
        }
    }

    /// The name `show ip pim interface` prints.
    pub fn label(self) -> &'static str {
        match self {
            PimMode::Sparse => "Sparse",
            PimMode::Dense => "Dense",
            PimMode::SparseDense => "Sparse-dense",
        }
    }
}

/// PIM on one interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PimIface {
    pub mode: PimMode,
    /// The priority in the designated router election (higher wins; 1 by default).
    #[serde(default = "default_priority")]
    pub dr_priority: u32,
    /// `ip multicast ttl-threshold N`: packets whose TTL is N or less are not sent out of the
    /// interface (0: no limit).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ttl_threshold: u8,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

fn default_priority() -> u32 {
    1
}

impl PimIface {
    pub fn new(mode: PimMode) -> PimIface {
        PimIface { mode, dr_priority: default_priority(), ttl_threshold: 0 }
    }
}

/// `ip pim rp-address A [ACL]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpLine {
    pub rp: Ipv4Addr,
    /// The standard access list naming the groups this rendezvous point serves (none: all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acl: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pim {
    /// `ip multicast-routing`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub routing: bool,
    /// Per interface, by its name (`GigabitEthernet0/0`, `Vlan10`, `Loopback0`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub ifaces: BTreeMap<String, PimIface>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rps: Vec<RpLine>,
    /// `ip igmp join-group`: per interface, the groups the router itself is a member of.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub joins: BTreeMap<String, BTreeSet<Ipv4Addr>>,
}

impl Pim {
    /// Whether the device holds no multicast setting at all.
    pub fn is_empty(&self) -> bool {
        *self == Pim::default()
    }

    /// PIM on an interface, if it runs there.
    pub fn iface(&self, name: &str) -> Option<&PimIface> {
        self.ifaces.get(name)
    }

    /// The rendezvous point that serves a group: a line whose access list permits it first,
    /// else the first line without a list.
    pub fn rp_for(&self, acls: &[Acl], group: Ipv4Addr) -> Option<Ipv4Addr> {
        let listed = self.rps.iter().find(|r| {
            r.acl
                .as_ref()
                .is_some_and(|n| acls.iter().find(|a| a.name == *n).is_some_and(|a| a.evaluate(&PacketKey::bare(0, group, Ipv4Addr::UNSPECIFIED)).0))
        });
        listed.or_else(|| self.rps.iter().find(|r| r.acl.is_none())).map(|r| r.rp)
    }

    /// Whether any interface of the router has joined the group.
    pub fn has_joined(&self, group: Ipv4Addr) -> bool {
        self.joins.values().any(|g| g.contains(&group))
    }

    /// Whether an address is a group a router interface may join (not a link-local one).
    pub fn joinable(group: Ipv4Addr) -> bool {
        crate::igmp::Igmp::joinable(group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_goes_to_the_rp_whose_list_permits_it() {
        let mut p = Pim::default();
        p.rps.push(RpLine { rp: Ipv4Addr::new(1, 1, 1, 1), acl: None });
        p.rps.push(RpLine { rp: Ipv4Addr::new(2, 2, 2, 2), acl: Some("G".into()) });
        let mut acl = Acl::new("G", false);
        acl.entries.push(crate::acl::AclEntry::new(
            true,
            crate::acl::AclProto::Ip,
            crate::acl::AclAddr { addr: Ipv4Addr::new(239, 0, 0, 0), wildcard: Ipv4Addr::new(0, 255, 255, 255) },
            crate::acl::AclAddr::ANY,
        ));
        let acls = vec![acl];
        assert_eq!(p.rp_for(&acls, Ipv4Addr::new(239, 1, 1, 1)), Some(Ipv4Addr::new(2, 2, 2, 2)));
        assert_eq!(p.rp_for(&acls, Ipv4Addr::new(224, 5, 5, 5)), Some(Ipv4Addr::new(1, 1, 1, 1)));
        assert_eq!(Pim::default().rp_for(&acls, Ipv4Addr::new(239, 1, 1, 1)), None);
    }
}
