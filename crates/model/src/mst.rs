//! Multiple spanning tree settings of a switch (`spanning-tree mode mst`, `spanning-tree mst
//! configuration`): the region's name and revision, which VLANs belong to which instance, and the
//! bridge priority of each instance.
//!
//! Instance 0 is the CIST and holds every VLAN that no other instance claims. Two switches are in
//! the same region when they run MST and their name, revision and VLAN-to-instance table agree.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The highest instance number (instances 0-15 exist, as on a small switch).
pub const MAX_INSTANCE: u16 = 15;
/// Longest region name.
pub const MAX_NAME: usize = 32;
/// The bridge priority an instance has unless one is set.
pub const DEFAULT_PRIORITY: u16 = 32_768;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Mst {
    /// `spanning-tree mode mst`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub on: bool,
    /// `name NAME` in `spanning-tree mst configuration`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// `revision N`.
    #[serde(skip_serializing_if = "is_zero")]
    pub revision: u16,
    /// `instance N vlan LIST`: the VLANs each instance (1-15) holds.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub instances: BTreeMap<u16, BTreeSet<u16>>,
    /// `spanning-tree mst N priority P`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub priority: BTreeMap<u16, u16>,
}

fn is_zero(n: &u16) -> bool {
    *n == 0
}

impl Mst {
    /// The instance a VLAN belongs to (0, the CIST, unless an instance claims it).
    pub fn instance_of(&self, vlan: u16) -> u16 {
        self.instances.iter().find(|(_, v)| v.contains(&vlan)).map_or(0, |(i, _)| *i)
    }

    /// The bridge priority of an instance.
    pub fn priority_of(&self, instance: u16) -> u16 {
        self.priority.get(&instance).copied().unwrap_or(DEFAULT_PRIORITY)
    }

    /// Whether the two switches are in one region: both run MST with the same name, revision and
    /// VLAN-to-instance table.
    pub fn same_region(&self, other: &Mst) -> bool {
        let table = |m: &Mst| -> BTreeMap<u16, u16> { m.instances.iter().flat_map(|(i, vs)| vs.iter().map(move |v| (*v, *i))).collect() };
        self.on && other.on && self.name == other.name && self.revision == other.revision && table(self) == table(other)
    }

    /// Instances that hold at least one VLAN, in order.
    pub fn used_instances(&self) -> Vec<u16> {
        self.instances.iter().filter(|(_, v)| !v.is_empty()).map(|(i, _)| *i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(name: &str, rev: u16, map: &[(u16, &[u16])]) -> Mst {
        Mst {
            on: true,
            name: name.into(),
            revision: rev,
            instances: map.iter().map(|(i, v)| (*i, v.iter().copied().collect())).collect(),
            priority: BTreeMap::new(),
        }
    }

    #[test]
    fn unclaimed_vlans_belong_to_the_cist() {
        let m = region("A", 1, &[(1, &[10, 20]), (2, &[30])]);
        assert_eq!((m.instance_of(10), m.instance_of(30), m.instance_of(40)), (1, 2, 0));
        assert_eq!(m.used_instances(), vec![1, 2]);
        assert_eq!(m.priority_of(1), DEFAULT_PRIORITY);
    }

    #[test]
    fn a_region_is_name_revision_and_table() {
        let a = region("A", 1, &[(1, &[10])]);
        assert!(a.same_region(&region("A", 1, &[(1, &[10])])));
        assert!(!a.same_region(&region("B", 1, &[(1, &[10])])));
        assert!(!a.same_region(&region("A", 2, &[(1, &[10])])));
        assert!(!a.same_region(&region("A", 1, &[(1, &[11])])));
        // An empty instance changes nothing; a switch not running MST is in no region.
        assert!(a.same_region(&region("A", 1, &[(1, &[10]), (2, &[])])));
        assert!(!a.same_region(&Mst { on: false, ..a.clone() }));
    }
}
