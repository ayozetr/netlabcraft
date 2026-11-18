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
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use netlabcraft_core::{DeviceId, Ipv6Net};
use netlabcraft_model::bgp as conf;
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::bgp as msg;

use crate::bgp::SessionState;
use crate::v6::{L3If6, Route6Entry, routes6, up_ifaces6};

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
    /// Both ends announced the multiprotocol capability for IPv6 unicast, so prefixes are exchanged.
    pub carries6: bool,
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

/// Most routers a TCP connection is followed through.
const MAX_HOPS: u8 = 16;

/// One side of an IPv6 session that can come up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peering6 {
    pub peer: DeviceId,
    /// How the other router names this one (its neighbour statement).
    pub peer_key: Peer,
    /// The neighbour's address, as this router configured it.
    pub peer_addr: Ipv6Addr,
    /// This router's address on the session: what the neighbour configured for it.
    pub local: Ipv6Addr,
    /// This router's link-local address on the interface the session leaves by.
    pub local_link: Ipv6Addr,
    pub out_iface: String,
    pub next_hop: Ipv6Addr,
    pub ebgp: bool,
}

/// What the lab allows for the sessions that run over IPv6, before any time has passed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Analysis6 {
    pub peerings: BTreeMap<Sess6, Peering6>,
    /// A route to the neighbour's address exists.
    pub routed: BTreeSet<Sess6>,
    /// The connection works but the OPEN messages are refused.
    pub errors: BTreeMap<Sess6, String>,
    /// The two ends do not have the same `password`.
    pub auth_failed: BTreeSet<Sess6>,
}

#[derive(Debug, Clone)]
struct Hop6 {
    owner: DeviceId,
    hops: u8,
    out_iface: String,
    next_hop: Ipv6Addr,
    /// This router's address on the interface it leaves by.
    egress: Ipv6Addr,
    egress_link: Ipv6Addr,
}

/// Follows a datagram from a router to the owner of `dst` through the IPv6 routing tables.
fn path_to6(t: &Topology, base: &BTreeMap<DeviceId, Vec<Route6Entry>>, from: &Device, dst: Ipv6Addr) -> Option<Hop6> {
    let mut cur = from;
    let mut first: Option<(String, Ipv6Addr, Ipv6Addr, Ipv6Addr)> = None;
    for hops in 0..MAX_HOPS {
        let ifs = up_ifaces6(t, cur);
        if ifs.iter().any(|i| i.owns(dst)) {
            let (out_iface, next_hop, egress, egress_link) = first?;
            return Some(Hop6 { owner: cur.id, hops, out_iface, next_hop, egress, egress_link });
        }
        if hops > 0 && !routes6(cur) {
            return None;
        }
        let table = base.get(&cur.id)?;
        let (name, nh, _) = resolve6(table, dst)?;
        let out = ifs.iter().find(|i| i.name == name)?;
        if first.is_none() {
            first = Some((name.clone(), nh, out.source_for(nh), out.link_local));
        }
        let reach = crate::ospf6::reach(t, cur, out);
        cur =
            t.devices.iter().find(|b| b.id != cur.id && up_ifaces6(t, b).iter().any(|ib| ib.owns(nh) && crate::ospf6::reaches(t, &reach, b, ib)))?;
    }
    None
}

fn multihop(n: &conf::Neighbor) -> u8 {
    n.ebgp_multihop.unwrap_or(1).max(1)
}

enum Attempt6 {
    No,
    BadAs(Sess6, bool),
    BadAuth(Sess6),
    Ok(Box<Peering6>, Box<Peering6>, Sess6),
}

fn near6(t: &Topology, d: &Device, addr: Ipv6Addr) -> bool {
    up_ifaces6(t, d).iter().any(|i| !crate::loopback::is_loopback(i.port) && i.addresses.iter().any(|n| n.contains(addr)))
}

fn attempt6(t: &Topology, base: &BTreeMap<DeviceId, Vec<Route6Entry>>, a: &Device, ab: &conf::Bgp, n: &conf::Neighbor, to: &Hop6) -> Attempt6 {
    let Some(peer_addr) = n.addr6 else { return Attempt6::No };
    let Some(b) = t.device(to.owner).filter(|b| routes6(b)) else { return Attempt6::No };
    let Some(bb) = &b.bgp else { return Attempt6::No };
    let ifs: Vec<L3If6> = up_ifaces6(t, a);
    let src = match &n.update_source {
        Some(name) => match ifs.iter().find(|i| i.name.eq_ignore_ascii_case(name)).and_then(|i| i.addresses.first()) {
            Some(x) => x.addr,
            None => return Attempt6::No,
        },
        None => to.egress,
    };
    let Some(m) = bb.neighbor6(src).filter(|m| !m.shutdown && m.remote_as.is_some()) else { return Attempt6::No };
    let Some(back) = path_to6(t, base, b, src).filter(|h| h.owner == a.id) else { return Attempt6::No };
    let bkey = (b.id, Peer::V6(src));
    if n.password != m.password {
        return Attempt6::BadAuth(bkey);
    }
    let (their_as, my_as) = (bb.outer_as(), ab.outer_as());
    let (mine_ok, theirs_ok) = (n.remote_as == Some(their_as), m.remote_as == Some(my_as));
    if !mine_ok || !theirs_ok {
        return Attempt6::BadAs(bkey, !mine_ok);
    }
    let ebgp = my_as != their_as;
    if ebgp
        && (to.hops > multihop(n)
            || (multihop(n) == 1 && !near6(t, a, peer_addr))
            || back.hops > multihop(m)
            || (multihop(m) == 1 && !near6(t, b, src)))
    {
        return Attempt6::No;
    }
    let mine = Peering6 {
        peer: b.id,
        peer_key: Peer::V6(src),
        peer_addr,
        local: src,
        local_link: to.egress_link,
        out_iface: to.out_iface.clone(),
        next_hop: to.next_hop,
        ebgp,
    };
    let theirs = Peering6 {
        peer: a.id,
        peer_key: Peer::V6(peer_addr),
        peer_addr: src,
        local: peer_addr,
        local_link: back.egress_link,
        out_iface: back.out_iface.clone(),
        next_hop: back.next_hop,
        ebgp,
    };
    Attempt6::Ok(Box::new(mine), Box::new(theirs), bkey)
}

/// Which sessions over IPv6 can come up in the lab as it is, given every router's IPv6 routing
/// table without BGP's routes.
pub fn analyse6(t: &Topology, base: &BTreeMap<DeviceId, Vec<Route6Entry>>) -> Analysis6 {
    let mut an = Analysis6::default();
    for a in t.devices.iter().filter(|d| routes6(d)) {
        let Some(ab) = &a.bgp else { continue };
        for n in &ab.neighbors6 {
            let Some(addr) = n.addr6 else { continue };
            if n.shutdown || n.remote_as.is_none() {
                continue;
            }
            let key = (a.id, Peer::V6(addr));
            let Some(to) = path_to6(t, base, a, addr) else { continue };
            an.routed.insert(key);
            match attempt6(t, base, a, ab, n, &to) {
                Attempt6::Ok(mine, theirs, bkey) => {
                    an.peerings.entry(key).or_insert(*mine);
                    an.peerings.entry(bkey).or_insert(*theirs);
                }
                Attempt6::BadAs(bkey, mine_refuses) => {
                    let (here, there) = if mine_refuses { ("sent", "received") } else { ("received", "sent") };
                    let why = msg::notification_text(msg::ERR_OPEN, msg::OPEN_BAD_PEER_AS);
                    an.errors.entry(key).or_insert_with(|| format!("{here} NOTIFICATION: {why}"));
                    an.errors.entry(bkey).or_insert_with(|| format!("{there} NOTIFICATION: {why}"));
                }
                Attempt6::BadAuth(bkey) => {
                    an.auth_failed.insert(key);
                    an.auth_failed.insert(bkey);
                }
                Attempt6::No => {}
            }
        }
    }
    an
}

/// What the IPv6 side of BGP depends on besides what the IPv4 side already hashes: the IPv6
/// addressing and routes of every router, its IPv6 routing protocols and its IPv6 policy.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (crate::ospf6::fingerprint(t), crate::eigrp6::fingerprint(t), crate::rip6::fingerprint(t)).hash(&mut h);
    for d in t.devices.iter().filter(|d| d.bgp.is_some()) {
        let ports: Vec<String> =
            d.ports.iter().map(|p| format!("{:?}{:?}{}", p.ipv6, p.subifs.iter().map(|s| &s.ipv6).collect::<Vec<_>>(), p.shutdown)).collect();
        let svis: Vec<String> = d.svis.iter().map(|s| format!("{:?}{}", s.ipv6, s.shutdown)).collect();
        format!("{:?}{:?}{:?}{:?}{:?}{:?}{:?}", d.id, d.ipv6_routing, d.routes6, d.prefix_lists6, d.route_maps, ports, svis).hash(&mut h);
    }
    h.finish()
}

/// The IPv6 unicast view of every router: its neighbours that are activated in the family, with the
/// state of the session they run over (an IPv4 session's state comes from the IPv4 tables `st4`).
pub fn compute6(t: &Topology, st4: &crate::bgp::BgpState, an6: &Analysis6, settled6: &BTreeSet<Sess6>) -> Bgp6State {
    let mut st = Bgp6State::default();
    for d in t.devices.iter().filter(|d| d.bgp.is_some() && routes6(d)) {
        let Some(ab) = &d.bgp else { continue };
        let Some(v4) = st4.routers.get(&d.id) else { continue };
        let mut neighbors = Vec::new();
        for n in ab.neighbors.iter().filter(|n| n.af6.activated) {
            let Some(v) = v4.neighbors.iter().find(|v| v.addr == n.addr) else { continue };
            let theirs = v.peer.and_then(|pd| t.device(pd)).and_then(|pd| pd.bgp.as_ref()).and_then(|pb| v.local.and_then(|l| pb.neighbor(l)));
            neighbors.push(NeighborView6 {
                peer: Peer::V4(n.addr),
                remote_as: n.remote_as,
                state: v.state,
                admin_down: v.admin_down,
                peer_dev: v.peer,
                peer_router_id: v.peer_router_id,
                transport6: false,
                carries6: theirs.is_some_and(|m| m.af6.activated),
                ebgp: v.ebgp,
                error: v.error.clone(),
                received: 0,
                sent: 0,
            });
        }
        for n in ab.neighbors6.iter().filter(|n| n.af6.activated) {
            let Some(addr) = n.addr6 else { continue };
            let key = (d.id, Peer::V6(addr));
            let peering = an6.peerings.get(&key);
            let up = peering.is_some_and(|p| settled6.contains(&key) && settled6.contains(&(p.peer, p.peer_key)));
            let state = if n.shutdown || n.remote_as.is_none() {
                SessionState::Idle
            } else if up {
                SessionState::Established
            } else if peering.is_some() {
                SessionState::Connect
            } else if an6.errors.contains_key(&key) || an6.auth_failed.contains(&key) {
                SessionState::Idle
            } else if an6.routed.contains(&key) {
                SessionState::Active
            } else {
                SessionState::Idle
            };
            let error = if n.shutdown {
                None
            } else if n.remote_as.is_none() {
                Some("no remote AS configured".to_string())
            } else if an6.auth_failed.contains(&key) {
                Some(crate::bgp::AUTH_FAILED.to_string())
            } else {
                an6.errors.get(&key).cloned()
            };
            let their = peering.and_then(|p| t.device(p.peer).and_then(|pd| pd.bgp.as_ref()).and_then(|pb| pb.neighbor6(p.local)));
            neighbors.push(NeighborView6 {
                peer: Peer::V6(addr),
                remote_as: n.remote_as,
                state,
                admin_down: n.shutdown,
                peer_dev: peering.map(|p| p.peer),
                peer_router_id: peering.map(|p| router_id_of(t, p.peer)),
                transport6: true,
                carries6: their.is_some_and(|m| m.af6.activated),
                ebgp: peering.is_some_and(|p| p.ebgp),
                error,
                received: 0,
                sent: 0,
            });
        }
        let router_id = crate::bgp::router_id(d);
        st.routers.insert(d.id, RouterView6 { asn: ab.asn, router_id, neighbors, paths: Vec::new(), routes: Vec::new(), adv: BTreeMap::new() });
    }
    st
}

fn router_id_of(t: &Topology, id: DeviceId) -> Ipv4Addr {
    t.device(id).map_or(Ipv4Addr::UNSPECIFIED, crate::bgp::router_id)
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
