//! Object tracking (`track N …`): numbered objects that watch an interface, a route, an IP SLA
//! operation or a list of other objects, and say whether the thing is up. Static routes
//! (`ip route … track N`) and first-hop redundancy groups (`standby N track M decrement D`,
//! `vrrp N track M`) follow an object; what an object reports is computed by the simulator
//! (`netlabcraft_sim::track`), and the state it settled on after its delays is kept here.

use std::collections::{BTreeMap, BTreeSet};

use netlabcraft_core::Ipv4Net;
use serde::{Deserialize, Serialize};

use crate::NextHop;

/// Most tracked objects on one device.
pub const MAX_OBJECTS: usize = 128;
/// Highest object number.
pub const MAX_ID: u32 = 1000;
/// Most members in a list object.
pub const MAX_MEMBERS: usize = 16;
/// Longest delay, in seconds.
pub const MAX_DELAY: u32 = 180;
/// Most routes that follow an object.
pub const MAX_BOUND_ROUTES: usize = 512;

/// One member of a list object: another object, possibly negated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub not: bool,
}

/// What an object watches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// `track N interface IF line-protocol` (up while the interface works) or `… ip routing`
    /// (up while it works, has an address and the device routes).
    Interface { name: String, ip_routing: bool },
    /// `track N ip route A.B.C.D/M reachability`: up while the routing table has the prefix.
    Route { net: Ipv4Net },
    /// `track N ip sla M reachability|state`: up while the operation's last probe got through
    /// (`reachability` also counts a probe over its threshold).
    Sla { op: u32, state: bool },
    /// `track N list boolean and|or`: up while all (or any) of the members are up.
    List { and: bool, members: Vec<Member> },
}

/// A tracked object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Object {
    pub kind: Kind,
    /// `delay up S`: seconds a change to up has to last before it is reported.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delay_up: u32,
    /// `delay down S`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delay_down: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Object {
    pub fn new(kind: Kind) -> Object {
        Object { kind, delay_up: 0, delay_down: 0 }
    }
}

/// The state an object last settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reported {
    pub up: bool,
    /// Simulated time of the last change, microseconds.
    pub since_us: u64,
    pub changes: u32,
}

/// The outcome of an IP SLA operation's last probe, as the simulator wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlaOutcome {
    Ok,
    OverThreshold,
    Failed,
}

/// A device's tracked objects and what follows them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub objects: BTreeMap<u32, Object>,
    /// The settled state of each object (after its delays); absent until the simulator has
    /// looked, and then the live state is used.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub reported: BTreeMap<u32, Reported>,
    /// A change waiting out its delay: the new state and when it was first seen.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pending: BTreeMap<u32, (bool, u64)>,
    /// The last probe of each IP SLA operation.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sla_outcome: BTreeMap<u32, SlaOutcome>,
    /// Static routes that follow an object, by [`route_key`].
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub route_track: BTreeMap<String, u32>,
    /// Static routes that stay in the table when their next hop or interface is gone.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub permanent: BTreeSet<String>,
}

/// The key a static route is known by in [`Track::route_track`] and [`Track::permanent`].
pub fn route_key(net: &Ipv4Net, via: &NextHop) -> String {
    match via {
        NextHop::Address(a) => format!("{net} {a}"),
        NextHop::Interface(n) => format!("{net} {n}"),
    }
}

impl Track {
    pub fn is_empty(&self) -> bool {
        *self == Track::default()
    }

    /// The object a static route follows, if any.
    pub fn follows(&self, net: &Ipv4Net, via: &NextHop) -> Option<u32> {
        self.route_track.get(&route_key(net, via)).copied()
    }

    pub fn is_permanent(&self, net: &Ipv4Net, via: &NextHop) -> bool {
        self.permanent.contains(&route_key(net, via))
    }

    /// Forgets what an edited object settled on (its delay clock starts again).
    pub fn forget_state(&mut self, id: u32) {
        self.reported.remove(&id);
        self.pending.remove(&id);
    }

    /// Removes an object and what refers to it: list members and routes that follow it.
    pub fn remove(&mut self, id: u32) -> bool {
        let had = self.objects.remove(&id).is_some();
        self.forget_state(id);
        self.route_track.retain(|_, n| *n != id);
        for o in self.objects.values_mut() {
            if let Kind::List { members, .. } = &mut o.kind {
                members.retain(|m| m.id != id);
            }
        }
        had
    }

    /// Caps every table (a hostile file) and drops what refers to nothing sensible.
    pub fn sanitize(&mut self) {
        self.objects.retain(|id, _| (1..=MAX_ID).contains(id));
        while self.objects.len() > MAX_OBJECTS {
            self.objects.pop_last();
        }
        for o in self.objects.values_mut() {
            o.delay_up = o.delay_up.min(MAX_DELAY);
            o.delay_down = o.delay_down.min(MAX_DELAY);
            if let Kind::List { members, .. } = &mut o.kind {
                members.truncate(MAX_MEMBERS);
                members.retain(|m| (1..=MAX_ID).contains(&m.id));
            }
        }
        let ids: BTreeSet<u32> = self.objects.keys().copied().collect();
        self.reported.retain(|id, _| ids.contains(id));
        self.pending.retain(|id, _| ids.contains(id));
        self.sla_outcome.retain(|id, _| (1..=MAX_ID).contains(id));
        while self.sla_outcome.len() > MAX_OBJECTS {
            self.sla_outcome.pop_last();
        }
        self.route_track.retain(|_, n| (1..=MAX_ID).contains(n));
        while self.route_track.len() > MAX_BOUND_ROUTES {
            self.route_track.pop_last();
        }
        while self.permanent.len() > MAX_BOUND_ROUTES {
            self.permanent.pop_last();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_an_object_removes_what_refers_to_it() {
        let mut t = Track::default();
        t.objects.insert(1, Object::new(Kind::Sla { op: 1, state: false }));
        t.objects.insert(2, Object::new(Kind::List { and: true, members: vec![Member { id: 1, not: false }, Member { id: 3, not: true }] }));
        t.route_track.insert("0.0.0.0/0 10.0.0.2".into(), 1);
        t.reported.insert(1, Reported { up: true, since_us: 0, changes: 0 });
        assert!(t.remove(1));
        assert!(!t.remove(1));
        assert!(t.route_track.is_empty() && t.reported.is_empty());
        assert!(matches!(&t.objects[&2].kind, Kind::List { members, .. } if members.len() == 1));
    }

    #[test]
    fn sanitize_caps_hostile_tables() {
        let mut t = Track::default();
        for id in 0..2_000u32 {
            t.objects.insert(id, Object::new(Kind::Sla { op: id, state: true }));
        }
        t.objects.insert(
            5,
            Object { kind: Kind::List { members: vec![Member { id: 0, not: false }; 50], and: false }, delay_up: 9_999, delay_down: 9_999 },
        );
        t.sanitize();
        assert!(t.objects.len() <= MAX_OBJECTS && !t.objects.contains_key(&0));
        let o = &t.objects[&5];
        assert_eq!((o.delay_up, o.delay_down), (MAX_DELAY, MAX_DELAY));
        assert!(matches!(&o.kind, Kind::List { members, .. } if members.is_empty()));
    }

    #[test]
    fn route_keys_tell_next_hops_from_interfaces() {
        let net: Ipv4Net = "10.0.0.0/24".parse().unwrap();
        let a = route_key(&net, &NextHop::Address("10.1.1.1".parse().unwrap()));
        let b = route_key(&net, &NextHop::Interface("GigabitEthernet0/0".into()));
        assert_ne!(a, b);
    }
}
