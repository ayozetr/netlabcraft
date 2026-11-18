//! BGP for IPv6 unicast (MP-BGP, RFC 4760), worked out next to the IPv4 side in [`crate::bgp`].
//!
//! This module holds the IPv6 paths and tables: a path to an IPv6 prefix with the attributes the
//! decision process reads, the routes BGP gives the IPv6 routing table and what the console shows.
//! The IPv4 types stay as they are; nothing here changes how IPv4 routes are worked out.
//!
//! A neighbour is named by the address the router configured, which is an IPv4 address (the session
//! runs over IPv4 and also carries IPv6 prefixes) or an IPv6 address (the session runs over IPv6);
//! [`Peer`] is that name.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use netlabcraft_core::{DeviceId, Ipv6Net};
use netlabcraft_wire::bgp as msg;

use crate::bgp::SessionState;

/// How a router names a neighbour: by its IPv4 address or by its IPv6 address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Peer {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Peer::V4(a) => write!(f, "{a}"),
            Peer::V6(a) => write!(f, "{a}"),
        }
    }
}

/// A session as one router sees it: the router and the neighbour it configured.
pub type Sess6 = (DeviceId, Peer);

/// A path to an IPv6 prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path6 {
    pub net: Ipv6Net,
    /// The global next hop (`::` for a path this router originates).
    pub next_hop: Ipv6Addr,
    /// The link-local next hop that came with it, when the neighbour is on a shared link.
    pub link_local: Option<Ipv6Addr>,
    pub as_path: Vec<u32>,
    pub origin: u8,
    pub med: u32,
    pub local_pref: u32,
    pub weight: u32,
    /// The neighbour the path came from; `None` for a local one.
    pub from: Option<Peer>,
    /// That neighbour's router id.
    pub from_id: Ipv4Addr,
    /// Learned from an external neighbour.
    pub ebgp: bool,
    /// The next hop resolves in the IPv6 routing table.
    pub valid: bool,
    pub best: bool,
    /// The interface and next hop the next hop resolves to.
    pub resolved: Option<(String, Ipv6Addr)>,
    pub communities: Vec<u32>,
    pub ext_communities: Vec<u64>,
    pub as_set: Vec<u32>,
    /// The metric of the route that resolves the next hop (0 for a connected one).
    pub igp: u32,
    /// One of the equal paths `maximum-paths` puts in the routing table besides the best.
    pub multi: bool,
    /// An aggregate with `summary-only` keeps this path from being advertised.
    pub suppressed: bool,
    pub originator_id: Option<Ipv4Addr>,
    pub cluster_list: Vec<Ipv4Addr>,
    /// Made by an `aggregate-address`.
    pub aggregate: bool,
}

impl Path6 {
    /// A path with the usual starting values for `net` (nothing set, not yet usable).
    pub fn new(net: Ipv6Net) -> Path6 {
        Path6 {
            net,
            next_hop: Ipv6Addr::UNSPECIFIED,
            link_local: None,
            as_path: Vec::new(),
            origin: msg::ORIGIN_IGP,
            med: 0,
            local_pref: 100,
            weight: 0,
            from: None,
            from_id: Ipv4Addr::UNSPECIFIED,
            ebgp: false,
            valid: false,
            best: false,
            resolved: None,
            communities: Vec::new(),
            ext_communities: Vec::new(),
            as_set: Vec::new(),
            igp: 0,
            multi: false,
            suppressed: false,
            originator_id: None,
            cluster_list: Vec::new(),
            aggregate: false,
        }
    }

    /// Learned from an internal neighbour.
    pub fn internal(&self) -> bool {
        self.from.is_some() && !self.ebgp
    }

    /// The AS path length best-path selection compares: an AS set counts as one.
    pub fn path_len(&self) -> usize {
        self.as_path.len() + usize::from(!self.as_set.is_empty())
    }
}

/// A route BGP gives the IPv6 routing table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route6 {
    pub net: Ipv6Net,
    /// The BGP next hop (what `show ipv6 route` names).
    pub next_hop: Ipv6Addr,
    /// Where the packets go: the next hop's next hop when it is not on a connected network.
    pub via: Ipv6Addr,
    pub iface: String,
    pub distance: u8,
    pub metric: u32,
}

/// A neighbour as `show bgp ipv6 unicast summary` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborView6 {
    pub peer: Peer,
    pub remote_as: Option<u32>,
    pub state: SessionState,
    pub admin_down: bool,
    pub peer_dev: Option<DeviceId>,
    pub peer_router_id: Option<Ipv4Addr>,
    /// The session runs over IPv6.
    pub transport6: bool,
    pub ebgp: bool,
    /// Why a session that cannot come up does not.
    pub error: Option<String>,
    pub received: usize,
    pub sent: usize,
}

/// One router's IPv6 unicast view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterView6 {
    pub asn: u32,
    pub router_id: Ipv4Addr,
    pub neighbors: Vec<NeighborView6>,
    /// Every path, a prefix after another, the best one first.
    pub paths: Vec<Path6>,
    pub routes: Vec<Route6>,
    /// What this router advertises to each Established neighbour.
    pub adv: BTreeMap<Peer, Vec<(Ipv6Net, msg::PathAttrs)>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bgp6State {
    pub routers: BTreeMap<DeviceId, RouterView6>,
}

/// What a router's settings change in the choice of the best path.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rank6 {
    /// `maximum-paths N`: equal external paths in the routing table.
    pub multi: usize,
    /// `bgp always-compare-med`.
    pub always_med: bool,
    /// `bgp bestpath as-path ignore`.
    pub ignore_len: bool,
}

/// `a` is a better path than `b` to the same prefix, by the same steps as for IPv4.
pub fn better6(a: &Path6, b: &Path6, o: &Rank6) -> bool {
    if a.valid != b.valid {
        return a.valid;
    }
    if a.weight != b.weight {
        return a.weight > b.weight;
    }
    if a.local_pref != b.local_pref {
        return a.local_pref > b.local_pref;
    }
    if a.from.is_none() != b.from.is_none() {
        return a.from.is_none();
    }
    if !o.ignore_len && a.path_len() != b.path_len() {
        return a.path_len() < b.path_len();
    }
    if a.origin != b.origin {
        return a.origin < b.origin;
    }
    if (o.always_med || a.as_path.first() == b.as_path.first()) && a.med != b.med {
        return a.med < b.med;
    }
    if a.ebgp != b.ebgp {
        return a.ebgp;
    }
    if a.igp != b.igp {
        return a.igp < b.igp;
    }
    let (ra, rb) = (a.originator_id.unwrap_or(a.from_id), b.originator_id.unwrap_or(b.from_id));
    if ra != rb {
        return ra < rb;
    }
    if a.cluster_list.len() != b.cluster_list.len() {
        return a.cluster_list.len() < b.cluster_list.len();
    }
    a.from < b.from
}

/// Whether `b` is as good as the best path `a` in every step before the router id, so the two can
/// share the load under `maximum-paths`.
fn equal_cost6(a: &Path6, b: &Path6, o: &Rank6) -> bool {
    a.valid
        && b.valid
        && a.from.is_some()
        && b.from.is_some()
        && a.ebgp == b.ebgp
        && a.weight == b.weight
        && a.local_pref == b.local_pref
        && a.as_path.first() == b.as_path.first()
        && (o.ignore_len || a.path_len() == b.path_len())
        && a.origin == b.origin
        && a.med == b.med
        && a.igp == b.igp
}

/// Groups paths by prefix, marks the best of each (when its next hop resolves) and puts it first;
/// up to `multi - 1` more external paths as good as the best, through a different first hop, are
/// marked to share the load.
pub fn rank6(cands: Vec<Path6>, o: &Rank6) -> Vec<Path6> {
    let mut groups: BTreeMap<Ipv6Net, Vec<Path6>> = BTreeMap::new();
    for c in cands {
        groups.entry(c.net).or_default().push(c);
    }
    let mut out = Vec::new();
    for mut g in groups.into_values() {
        let mut best = 0usize;
        for i in 1..g.len() {
            if let (Some(c), Some(b)) = (g.get(i), g.get(best))
                && better6(c, b, o)
            {
                best = i;
            }
        }
        if best < g.len() {
            g.swap(0, best);
        }
        if let Some(p) = g.first_mut() {
            p.best = p.valid;
        }
        if let Some(top) = g.first().filter(|p| p.best).cloned() {
            let multipath = if top.ebgp { o.multi } else { 1 };
            let mut used: Vec<Option<(String, Ipv6Addr)>> = vec![top.resolved.clone()];
            for p in g.iter_mut().skip(1) {
                if multipath > 1 && used.len() < multipath && equal_cost6(&top, p, o) && !used.contains(&p.resolved) {
                    p.multi = true;
                    used.push(p.resolved.clone());
                }
            }
        }
        out.extend(g);
    }
    out
}

/// Where a datagram to `ip` goes out of a router, from its IPv6 routing table without BGP's routes:
/// the interface, the address to send to and the metric of the route it went through (0 for a
/// connected network).
pub fn resolve6(base: &[crate::v6::Route6Entry], ip: Ipv6Addr) -> Option<(String, Ipv6Addr, u32)> {
    if base.iter().any(|r| r.code == 'L' && r.net.addr == ip) {
        return None;
    }
    let r = base
        .iter()
        .filter(|r| r.code != 'L' && r.net.contains(ip) && r.iface.as_deref() != Some(crate::eigrp::NULL0))
        .max_by_key(|r| (r.net.prefix, Reverse(r.distance)))?;
    let metric = 0;
    match (r.code, r.next_hop, &r.iface) {
        ('C', _, Some(i)) => Some((i.clone(), ip, metric)),
        (_, Some(v), Some(i)) => Some((i.clone(), v, metric)),
        (_, None, Some(i)) => Some((i.clone(), ip, metric)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v6::Route6Entry;

    fn net(s: &str) -> Ipv6Net {
        let (a, p) = s.split_once('/').unwrap_or((s, "128"));
        Ipv6Net::new(a.parse().unwrap(), p.parse().unwrap())
    }

    fn path(prefix: &str, as_path: &[u32], from: Option<Peer>) -> Path6 {
        let mut p = Path6::new(net(prefix));
        p.as_path = as_path.to_vec();
        p.from = from;
        p.ebgp = from.is_some();
        p.valid = true;
        p.resolved = Some(("Gi0/0".into(), "2001:db8:12::2".parse().unwrap()));
        p
    }

    #[test]
    fn peers_are_told_apart_and_printed_as_their_addresses() {
        let v4 = Peer::V4(Ipv4Addr::new(10, 0, 0, 2));
        let v6 = Peer::V6("2001:db8::2".parse().unwrap());
        assert_ne!(v4, v6);
        assert!(v4 < v6, "IPv4 neighbours sort first");
        assert_eq!(v4.to_string(), "10.0.0.2");
        assert_eq!(v6.to_string(), "2001:db8::2");
    }

    #[test]
    fn the_decision_order_is_the_one_ipv4_uses() {
        let o = Rank6::default();
        let a = Some(Peer::V6("2001:db8::2".parse().unwrap()));
        let b = Some(Peer::V6("2001:db8::3".parse().unwrap()));
        let short = path("2001:db8:1::/48", &[65_002], a);
        let long = path("2001:db8:1::/48", &[65_003, 65_004], b);
        assert!(better6(&short, &long, &o), "the shorter AS path wins");
        let mut heavy = long.clone();
        heavy.weight = 10;
        assert!(better6(&heavy, &short, &o), "weight comes first");
        let mut invalid = short.clone();
        invalid.valid = false;
        assert!(better6(&long, &invalid, &o), "a path whose next hop does not resolve loses");
        let local = path("2001:db8:1::/48", &[], None);
        assert!(better6(&local, &short, &o), "a path this router originates beats a learned one");
    }

    #[test]
    fn rank_marks_the_best_and_shares_the_load_only_up_to_maximum_paths() {
        let a = path("2001:db8:1::/48", &[65_002], Some(Peer::V6("2001:db8::2".parse().unwrap())));
        let mut b = path("2001:db8:1::/48", &[65_002], Some(Peer::V6("2001:db8::3".parse().unwrap())));
        b.resolved = Some(("Gi0/1".into(), "2001:db8:13::3".parse().unwrap()));
        let other = path("2001:db8:2::/48", &[65_009], Some(Peer::V4(Ipv4Addr::new(10, 0, 0, 9))));
        let one = rank6(vec![a.clone(), b.clone(), other.clone()], &Rank6 { multi: 1, ..Rank6::default() });
        assert_eq!(one.iter().filter(|p| p.best).count(), 2, "one best per prefix");
        assert!(one.iter().all(|p| !p.multi));
        let two = rank6(vec![a, b, other], &Rank6 { multi: 2, ..Rank6::default() });
        assert_eq!(two.iter().filter(|p| p.multi).count(), 1, "the second equal path shares the load");
    }

    #[test]
    fn a_next_hop_resolves_through_the_most_specific_route_and_not_through_a_null_route() {
        let route = |code: char, n: &str, iface: &str, nh: Option<&str>, distance: u8| Route6Entry {
            code,
            net: net(n),
            iface: Some(iface.into()),
            next_hop: nh.map(|x| x.parse().unwrap()),
            distance,
            tag: 0,
        };
        let table = vec![
            route('C', "2001:db8:12::/64", "Gi0/0", None, 0),
            route('L', "2001:db8:12::1/128", "Gi0/0", None, 0),
            route('S', "2001:db8:99::/48", "Gi0/1", Some("2001:db8:13::3"), 1),
            route('S', "2001:db8:77::/48", crate::eigrp::NULL0, None, 1),
        ];
        let got = |ip: &str| resolve6(&table, ip.parse().unwrap());
        assert_eq!(got("2001:db8:12::2"), Some(("Gi0/0".into(), "2001:db8:12::2".parse().unwrap(), 0)));
        assert_eq!(got("2001:db8:99::5"), Some(("Gi0/1".into(), "2001:db8:13::3".parse().unwrap(), 0)));
        assert_eq!(got("2001:db8:12::1"), None, "one of this router's own addresses");
        assert_eq!(got("2001:db8:77::1"), None, "a discard route does not resolve a next hop");
        assert_eq!(got("2001:dead::1"), None);
    }
}
