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
use netlabcraft_model::bgp::{self as conf, Source};
use netlabcraft_model::policy::{self, Sets};
use netlabcraft_model::policy6::{Subject6, apply_with6};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::bgp as msg;

use crate::bgp::{DISTANCE_EBGP, DISTANCE_IBGP, SessionState, WEIGHT_LOCAL};
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

/// Rounds of advertisements at most.
const MAX_ROUNDS: usize = 64;
/// Most paths a router keeps from its neighbours, and most it originates.
const MAX_PATHS: usize = 4_096;
const MAX_LOCAL: usize = 1_024;
/// Most AS numbers in the AS set of an aggregate.
const MAX_AS_SET: usize = 32;

/// One end of an IPv6 unicast exchange: a session that is up and that both ends activated for the
/// family, whichever transport it runs over.
#[derive(Debug, Clone)]
struct Link6 {
    /// How this router names the neighbour.
    key: Peer,
    peer: DeviceId,
    /// How the neighbour names this router.
    peer_key: Peer,
    ebgp: bool,
    /// The global next hop this router puts in what it sends.
    next_hop: Ipv6Addr,
    /// The link-local next hop that goes with it (external sessions on a shared link).
    link_local: Option<Ipv6Addr>,
    /// This router's settings for the neighbour, and the neighbour's for this router.
    n: conf::Neighbor,
    m: conf::Neighbor,
}

fn global_of(i: &L3If6) -> Option<Ipv6Addr> {
    i.addresses.iter().map(|a| a.addr).find(|a| !Ipv6Net::is_link_local(*a))
}

/// The sessions of every router that exchange IPv6 prefixes now.
fn links(
    t: &Topology,
    an: &crate::bgp::Analysis,
    settled: &BTreeSet<crate::bgp::SessKey>,
    an6: &Analysis6,
    settled6: &BTreeSet<Sess6>,
) -> BTreeMap<DeviceId, Vec<Link6>> {
    let mut out: BTreeMap<DeviceId, Vec<Link6>> = BTreeMap::new();
    for a in t.devices.iter().filter(|d| d.bgp.is_some() && routes6(d)) {
        let Some(ab) = &a.bgp else { continue };
        let ifs = up_ifaces6(t, a);
        for n in ab.neighbors.iter().filter(|n| n.af6.activated && !n.shutdown) {
            let key = (a.id, n.addr);
            let Some(p) = an.peerings.get(&key) else { continue };
            if p.confed || !settled.contains(&key) || !settled.contains(&(p.peer, p.local)) {
                continue;
            }
            let Some(b) = t.device(p.peer).filter(|b| routes6(b)) else { continue };
            let Some(m) = b.bgp.as_ref().and_then(|bb| bb.neighbor(p.local)).filter(|m| m.af6.activated && !m.shutdown) else { continue };
            // Over an IPv4 session the IPv6 next hop is this router's address on the interface the
            // session leaves by; without one it is the IPv4-mapped form, which no table resolves.
            let (next_hop, link_local) = match ifs.iter().find(|i| i.name == p.out_iface) {
                Some(i) => (global_of(i).unwrap_or_else(|| p.local.to_ipv6_mapped()), p.ebgp.then_some(i.link_local)),
                None => (p.local.to_ipv6_mapped(), None),
            };
            out.entry(a.id).or_default().push(Link6 {
                key: Peer::V4(n.addr),
                peer: p.peer,
                peer_key: Peer::V4(p.local),
                ebgp: p.ebgp,
                next_hop,
                link_local,
                n: n.clone(),
                m: m.clone(),
            });
        }
        for n in ab.neighbors6.iter().filter(|n| n.af6.activated && !n.shutdown) {
            let Some(addr) = n.addr6 else { continue };
            let key = (a.id, Peer::V6(addr));
            let Some(p) = an6.peerings.get(&key) else { continue };
            if !settled6.contains(&key) || !settled6.contains(&(p.peer, p.peer_key)) {
                continue;
            }
            let Some(b) = t.device(p.peer).filter(|b| routes6(b)) else { continue };
            let Some(m) = b.bgp.as_ref().and_then(|bb| bb.neighbor6(p.local)).filter(|m| m.af6.activated && !m.shutdown) else { continue };
            out.entry(a.id).or_default().push(Link6 {
                key: Peer::V6(addr),
                peer: p.peer,
                peer_key: p.peer_key,
                ebgp: p.ebgp,
                next_hop: p.local,
                link_local: p.ebgp.then_some(p.local_link),
                n: n.clone(),
                m: m.clone(),
            });
        }
    }
    out
}

fn neighbor_of(ab: &conf::Bgp, key: Peer) -> Option<&conf::Neighbor> {
    match key {
        Peer::V4(a) => ab.neighbor(a),
        Peer::V6(a) => ab.neighbor6(a),
    }
}

/// The paths a router starts from: its `network` prefixes that are in its IPv6 routing table, and
/// what it redistributes.
fn originate6(d: &Device, table: &[Route6Entry]) -> Vec<Path6> {
    let Some(b) = &d.bgp else { return Vec::new() };
    let usable = |r: &&Route6Entry| r.code != 'L' && r.iface.as_deref() != Some(crate::eigrp::NULL0);
    let mut nets: Vec<(Ipv6Net, u8, Sets)> = Vec::new();
    for n in &b.networks6 {
        if table.iter().filter(usable).any(|r| r.net == *n) && !nets.iter().any(|(x, ..)| x == n) {
            nets.push((*n, msg::ORIGIN_IGP, Sets::default()));
        }
    }
    for s in &b.redistribute6 {
        let codes: &[char] = match s {
            Source::Connected => &['C'],
            Source::Static => &['S'],
            Source::Ospf => &['O', 'I', 'E'],
        };
        let map = b.redistribute_maps6.get(s.word()).map(String::as_str);
        for r in table.iter().filter(usable).filter(|r| codes.contains(&r.code)) {
            if nets.iter().any(|(x, ..)| *x == r.net) {
                continue;
            }
            if let Some(sets) = apply_with6(d, None, map, &Subject6 { net: r.net, metric: 0 }, &policy::Attrs::default()) {
                nets.push((r.net, msg::ORIGIN_INCOMPLETE, sets));
            }
        }
    }
    nets.truncate(MAX_LOCAL);
    nets.into_iter()
        .map(|(net, origin, sets)| {
            let mut p = Path6::new(net);
            p.next_hop = sets.next_hop6.unwrap_or(Ipv6Addr::UNSPECIFIED);
            p.as_path = sets.prepend.clone();
            p.origin = sets.origin.unwrap_or(origin);
            p.med = sets.metric.unwrap_or(0);
            p.local_pref = sets.local_pref.unwrap_or_else(|| b.local_pref());
            p.weight = sets.weight.unwrap_or(WEIGHT_LOCAL);
            p.valid = true;
            p.communities = policy::merge_communities(&sets, &[]);
            p.ext_communities = policy::merge_ext_communities(&sets, &[]);
            p
        })
        .collect()
}

/// The paths an `aggregate-address` makes: one per statement that has a more specific usable path
/// under it (and no path of its own for the same prefix from a `network` statement).
fn aggregate_paths6(ab: &conf::Bgp, paths: &[Path6]) -> Vec<Path6> {
    let mut out = Vec::new();
    for g in &ab.aggregates6 {
        let under = |p: &&Path6| p.valid && p.net.prefix > g.net.prefix && g.net.contains(p.net.addr);
        if !paths.iter().any(|p| under(&p)) || paths.iter().any(|p| p.net == g.net && p.from.is_none()) {
            continue;
        }
        let mut set: Vec<u32> = Vec::new();
        if g.as_set {
            for p in paths.iter().filter(under) {
                set.extend(p.as_path.iter().chain(p.as_set.iter()).copied());
            }
            set.sort_unstable();
            set.dedup();
            set.truncate(MAX_AS_SET);
        }
        let mut p = Path6::new(g.net);
        p.local_pref = ab.local_pref();
        p.weight = WEIGHT_LOCAL;
        p.valid = true;
        p.as_set = set;
        p.aggregate = true;
        out.push(p);
    }
    out
}

/// A `summary-only` aggregate covers this prefix with a shorter one.
fn summarised6(ab: &conf::Bgp, net: Ipv6Net) -> bool {
    ab.aggregates6.iter().any(|g| g.summary_only && g.net.prefix < net.prefix && g.net.contains(net.addr))
}

fn attrs_of(p: &Path6) -> policy::Attrs<'_> {
    policy::Attrs { communities: &p.communities, as_path: &p.as_path, as_set: &p.as_set, tag: 0, ext_communities: &p.ext_communities }
}

/// What a router tells a neighbour about one of its best paths, if it tells it at all: the
/// neighbour's outbound prefix list and route map decide, and the map's `set` lines change what is
/// told. The prefix and the next hop travel in MP_REACH_NLRI.
fn export6(a: &Device, ab: &conf::Bgp, l: &Link6, path: &Path6) -> Option<msg::PathAttrs> {
    let n = &l.n;
    if path.from == Some(l.key) {
        return None;
    }
    if l.ebgp && n.remote_as.is_some_and(|r| path.as_path.contains(&r) || path.as_set.contains(&r)) {
        return None;
    }
    if path.communities.contains(&policy::NO_ADVERTISE) {
        return None;
    }
    if l.ebgp && (path.communities.contains(&policy::NO_EXPORT) || path.communities.contains(&policy::LOCAL_AS)) {
        return None;
    }
    if summarised6(ab, path.net) {
        return None;
    }
    if let Some(list) = n.af6.filter_list_out.as_deref().and_then(|x| a.as_path_list(x))
        && !list.permits(&path.as_path, &path.as_set)
    {
        return None;
    }
    // A path learned from an internal neighbour is not passed to another one, unless this router is
    // a route reflector (RFC 4456): a client's path goes to everybody, a non-client's to clients.
    let reflect = !l.ebgp && path.internal();
    if reflect {
        let from_client = path.from.and_then(|f| neighbor_of(ab, f)).is_some_and(|s| s.af6.route_reflector_client);
        if !from_client && !n.af6.route_reflector_client {
            return None;
        }
    }
    let sets = apply_with6(
        a,
        n.af6.prefix_list_out.as_deref(),
        n.af6.route_map_out.as_deref(),
        &Subject6 { net: path.net, metric: path.med },
        &attrs_of(path),
    )?;
    let communities = if n.af6.send_community { policy::merge_communities(&sets, &path.communities) } else { Vec::new() };
    let mut ext_communities = if n.af6.send_extended { policy::merge_ext_communities(&sets, &path.ext_communities) } else { Vec::new() };
    if l.ebgp {
        ext_communities.retain(|c| msg::ext_transitive(*c));
    }
    let mut tail = sets.prepend.clone();
    tail.extend(path.as_path.iter().copied());
    let origin = sets.origin.unwrap_or(path.origin);
    // A path this router originates names its own address, unless a map named another next hop.
    let own = if path.from.is_none() && !path.next_hop.is_unspecified() { path.next_hop } else { l.next_hop };
    let mp = |next_hop: Ipv6Addr, link_local: Option<Ipv6Addr>| msg::MpReach6 { next_hop, link_local, nlri: vec![path.net] };
    if l.ebgp {
        let mut as_path = vec![ab.outer_as()];
        as_path.extend(tail);
        return Some(msg::PathAttrs {
            origin,
            as_path,
            med: sets.metric.or_else(|| path.from.is_none().then_some(path.med)),
            as_set: path.as_set.clone(),
            communities,
            ext_communities,
            mp_reach6: Some(mp(sets.next_hop6.unwrap_or(own), if sets.next_hop6.is_some() { None } else { l.link_local })),
            ..msg::PathAttrs::default()
        });
    }
    let (originator_id, cluster_list) = if reflect {
        let mut clusters = vec![ab.cluster_id.unwrap_or_else(|| crate::bgp::router_id(a))];
        clusters.extend(path.cluster_list.iter().copied());
        clusters.truncate(msg::MAX_CLUSTERS);
        (Some(path.originator_id.unwrap_or(path.from_id)), clusters)
    } else {
        (None, Vec::new())
    };
    // A reflected path keeps its next hop, `next-hop-self` or not.
    let next_hop = if !reflect && (n.af6.next_hop_self || path.from.is_none()) { own } else { path.next_hop };
    Some(msg::PathAttrs {
        origin,
        as_path: tail,
        med: Some(sets.metric.unwrap_or(path.med)),
        local_pref: Some(sets.local_pref.unwrap_or(path.local_pref)),
        as_set: path.as_set.clone(),
        communities,
        originator_id,
        cluster_list,
        ext_communities,
        mp_reach6: Some(mp(sets.next_hop6.unwrap_or(next_hop), None)),
        ..msg::PathAttrs::default()
    })
}

/// Everything a router tells one neighbour: its best paths that [`export6`] lets through, and the
/// default route when the neighbour has `default-originate`.
fn export_all6(a: &Device, ab: &conf::Bgp, l: &Link6, best: &[&Path6]) -> Vec<(Ipv6Net, msg::PathAttrs)> {
    let mut out: Vec<(Ipv6Net, msg::PathAttrs)> = best.iter().filter_map(|x| export6(a, ab, l, x).map(|at| (x.net, at))).take(MAX_PATHS).collect();
    let default = Ipv6Net::new(Ipv6Addr::UNSPECIFIED, 0);
    if l.n.af6.default_originate && !out.iter().any(|(net, _)| net.prefix == 0) {
        let attrs = msg::PathAttrs {
            as_path: if l.ebgp { vec![ab.outer_as()] } else { Vec::new() },
            med: if l.ebgp { None } else { Some(0) },
            local_pref: if l.ebgp { None } else { Some(ab.local_pref()) },
            mp_reach6: Some(msg::MpReach6 { next_hop: l.next_hop, link_local: l.link_local, nlri: vec![default] }),
            ..msg::PathAttrs::default()
        };
        out.push((default, attrs));
    }
    out
}

/// What a router makes of what a neighbour told it: `None` when it drops it, by the loop checks or
/// by the neighbour's inbound prefix list, filter list and route map.
fn import6(b: &Device, rx: &Link6, attrs: &msg::PathAttrs, from_id: Ipv4Addr, net: Ipv6Net, own: &[L3If6]) -> Option<Path6> {
    let bb = b.bgp.as_ref()?;
    let (m, mp) = (&rx.n, attrs.mp_reach6.as_ref()?);
    let outer = bb.outer_as();
    if !bb.exchanges6(m) || attrs.as_path.contains(&outer) || attrs.as_set.contains(&outer) || own.iter().any(|i| i.owns(mp.next_hop)) {
        return None;
    }
    let cluster = bb.cluster_id.unwrap_or_else(|| crate::bgp::router_id(b));
    if !rx.ebgp && (attrs.originator_id == Some(crate::bgp::router_id(b)) || attrs.cluster_list.contains(&cluster)) {
        return None;
    }
    if let Some(list) = m.af6.filter_list_in.as_deref().and_then(|x| b.as_path_list(x))
        && !list.permits(&attrs.as_path, &attrs.as_set)
    {
        return None;
    }
    let med = attrs.med.unwrap_or(0);
    let seen = policy::Attrs {
        communities: &attrs.communities,
        as_path: &attrs.as_path,
        as_set: &attrs.as_set,
        tag: 0,
        ext_communities: &attrs.ext_communities,
    };
    let sets = apply_with6(b, m.af6.prefix_list_in.as_deref(), m.af6.route_map_in.as_deref(), &Subject6 { net, metric: med }, &seen)?;
    let local_pref = sets.local_pref.unwrap_or_else(|| if rx.ebgp { bb.local_pref() } else { attrs.local_pref.unwrap_or_else(|| bb.local_pref()) });
    let mut as_path = sets.prepend.clone();
    as_path.extend(attrs.as_path.iter().copied());
    let mut p = Path6::new(net);
    p.next_hop = sets.next_hop6.unwrap_or(mp.next_hop);
    p.link_local = mp.link_local;
    p.as_path = as_path;
    p.origin = sets.origin.unwrap_or(attrs.origin);
    p.med = sets.metric.unwrap_or(med);
    p.local_pref = local_pref;
    p.weight = sets.weight.unwrap_or_else(|| u32::from(m.af6.weight.unwrap_or(0)));
    p.from = Some(rx.key);
    p.from_id = from_id;
    p.ebgp = rx.ebgp;
    p.communities = policy::merge_communities(&sets, &attrs.communities);
    p.ext_communities = policy::merge_ext_communities(&sets, &attrs.ext_communities);
    p.as_set = attrs.as_set.clone();
    p.originator_id = if rx.ebgp { None } else { attrs.originator_id };
    p.cluster_list = if rx.ebgp { Vec::new() } else { attrs.cluster_list.clone() };
    Some(p)
}

type Rib6 = BTreeMap<DeviceId, BTreeMap<(Peer, Ipv6Net), Path6>>;

fn rank_of(ab: &conf::Bgp) -> Rank6 {
    Rank6 { multi: ab.multipath6(), always_med: ab.always_compare_med, ignore_len: ab.as_path_ignore }
}

/// Every router's IPv6 paths and routes, once the sessions in `settled` and `settled6` are
/// Established, and the state of every neighbour activated for the family (an IPv4 session's state
/// comes from the IPv4 tables `st4`).
pub fn compute6(
    t: &Topology,
    base6: &BTreeMap<DeviceId, Vec<Route6Entry>>,
    st4: &crate::bgp::BgpState,
    an: &crate::bgp::Analysis,
    settled: &BTreeSet<crate::bgp::SessKey>,
    an6: &Analysis6,
    settled6: &BTreeSet<Sess6>,
) -> Bgp6State {
    let routers: Vec<&Device> = t.devices.iter().filter(|d| d.bgp.is_some() && routes6(d)).collect();
    let lk = links(t, an, settled, an6, settled6);
    let ifs: BTreeMap<DeviceId, Vec<L3If6>> = routers.iter().map(|d| (d.id, up_ifaces6(t, d))).collect();
    let table_of = |id: DeviceId| base6.get(&id).map_or(&[][..], Vec::as_slice);
    let local: BTreeMap<DeviceId, Vec<Path6>> = routers.iter().map(|d| (d.id, originate6(d, table_of(d.id)))).collect();
    let candidates = |d: &Device, rib: &Rib6| -> Vec<Path6> {
        let table = table_of(d.id);
        let mut v: Vec<Path6> = local.get(&d.id).cloned().unwrap_or_default();
        for p in rib.get(&d.id).into_iter().flat_map(|m| m.values()) {
            let mut p = p.clone();
            let hop = if p.next_hop.is_unspecified() { None } else { resolve6(table, p.next_hop) };
            p.valid = hop.is_some();
            p.igp = hop.as_ref().map_or(0, |h| h.2);
            p.resolved = hop.map(|h| (h.0, h.1));
            v.push(p);
        }
        let Some(ab) = &d.bgp else { return rank6(v, &Rank6::default()) };
        let made = aggregate_paths6(ab, &v);
        v.extend(made);
        for p in &mut v {
            p.suppressed = summarised6(ab, p.net);
        }
        rank6(v, &rank_of(ab))
    };
    let mut rib: Rib6 = BTreeMap::new();
    for _ in 0..MAX_ROUNDS {
        let mut next: Rib6 = BTreeMap::new();
        for a in &routers {
            let Some(ab) = &a.bgp else { continue };
            let best: Vec<Path6> = candidates(a, &rib).into_iter().filter(|p| p.best).collect();
            let best_refs: Vec<&Path6> = best.iter().collect();
            let from_id = crate::bgp::router_id(a);
            for l in lk.get(&a.id).into_iter().flatten() {
                let Some(b) = routers.iter().find(|d| d.id == l.peer) else { continue };
                // The receiving end of this session is the link the peer holds towards this router.
                let Some(rx) = lk.get(&b.id).and_then(|v| v.iter().find(|r| r.peer == a.id && r.key == l.peer_key)) else { continue };
                let own = ifs.get(&b.id).map_or(&[][..], Vec::as_slice);
                for (net, attrs) in export_all6(a, ab, l, &best_refs) {
                    let Some(got) = import6(b, rx, &attrs, from_id, net, own) else { continue };
                    let slot = next.entry(b.id).or_default();
                    if slot.len() < MAX_PATHS {
                        slot.insert((l.peer_key, net), got);
                    }
                }
            }
        }
        if next == rib {
            break;
        }
        rib = next;
    }
    let mut st = Bgp6State::default();
    for d in &routers {
        let Some(ab) = &d.bgp else { continue };
        let Some(v4) = st4.routers.get(&d.id) else { continue };
        let paths = candidates(d, &rib);
        let mut routes: Vec<Route6> = paths
            .iter()
            .filter(|p| (p.best || p.multi) && p.from.is_some())
            .filter_map(|p| {
                let (iface, via) = p.resolved.clone()?;
                Some(Route6 {
                    net: p.net,
                    next_hop: p.next_hop,
                    via,
                    iface,
                    distance: if p.ebgp { DISTANCE_EBGP } else { DISTANCE_IBGP },
                    metric: p.med,
                })
            })
            .collect();
        // An aggregate leaves a discard route behind, so what no more specific route covers is dropped.
        routes.extend(paths.iter().filter(|p| p.aggregate && p.best).map(|p| Route6 {
            net: p.net,
            next_hop: Ipv6Addr::UNSPECIFIED,
            via: Ipv6Addr::UNSPECIFIED,
            iface: crate::eigrp::NULL0.to_string(),
            distance: DISTANCE_IBGP,
            metric: 0,
        }));
        let best_paths: Vec<&Path6> = paths.iter().filter(|x| x.best).collect();
        let received = |key: Peer| rib.get(&d.id).map_or(0, |m| m.keys().filter(|(k, _)| *k == key).count());
        let mut adv: BTreeMap<Peer, Vec<(Ipv6Net, msg::PathAttrs)>> = BTreeMap::new();
        let mut neighbors = Vec::new();
        for l in lk.get(&d.id).into_iter().flatten() {
            adv.insert(l.key, export_all6(d, ab, l, &best_paths));
        }
        for n in ab.neighbors.iter().filter(|n| n.af6.activated) {
            let Some(v) = v4.neighbors.iter().find(|v| v.addr == n.addr) else { continue };
            let key = Peer::V4(n.addr);
            let theirs = v.peer.and_then(|pd| t.device(pd)).and_then(|pd| pd.bgp.as_ref()).and_then(|pb| v.local.and_then(|x| pb.neighbor(x)));
            neighbors.push(NeighborView6 {
                peer: key,
                remote_as: n.remote_as,
                state: v.state,
                admin_down: v.admin_down,
                peer_dev: v.peer,
                peer_router_id: v.peer_router_id,
                transport6: false,
                carries6: theirs.is_some_and(|m| m.af6.activated),
                ebgp: v.ebgp,
                error: v.error.clone(),
                received: received(key),
                sent: adv.get(&key).map_or(0, Vec::len),
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
                peer: key.1,
                remote_as: n.remote_as,
                state,
                admin_down: n.shutdown,
                peer_dev: peering.map(|p| p.peer),
                peer_router_id: peering.map(|p| router_id_of(t, p.peer)),
                transport6: true,
                carries6: their.is_some_and(|m| m.af6.activated),
                ebgp: peering.is_some_and(|p| p.ebgp),
                error,
                received: received(key.1),
                sent: adv.get(&key.1).map_or(0, Vec::len),
            });
        }
        st.routers.insert(d.id, RouterView6 { asn: ab.asn, router_id: crate::bgp::router_id(d), neighbors, paths, routes, adv });
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
