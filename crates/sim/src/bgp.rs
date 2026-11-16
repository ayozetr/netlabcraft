//! BGP-4 (RFC 4271), worked out the way the other routing protocols here are: from the lab as it
//! is now, with the messages the speakers would send put on the wire.
//!
//! A *session* exists between two routers when one of them can open a TCP connection to the
//! other: the neighbour's address is reachable through the routing table without BGP's own
//! routes (connected, static, OSPF, EIGRP and RIP), the other router has a `neighbor` statement
//! for the address the connection comes from, the AS numbers agree, and an external peer is
//! close enough (`ebgp-multihop`). A session then spends [`SETUP_US`] of simulated time on its
//! way up (Connect, OpenSent, OpenConfirm) before it is Established and routes flow.
//!
//! Routes flow in rounds until nothing changes. Each router picks a best path per prefix from
//! what it originates and what its neighbours sent it, and advertises its best paths to its
//! Established neighbours: external neighbours see the router's AS added to the AS path and
//! the router's own address as the next hop; internal neighbours get the path as it is (the
//! next hop too, unless `next-hop-self`), and never a path learned from another internal
//! neighbour unless the router is a route reflector (RFC 4456: a path from a client goes to every
//! internal neighbour, one from a non-client only to the clients, with ORIGINATOR_ID and the
//! CLUSTER_LIST for loop prevention). A path whose AS path holds the receiver's AS is dropped.
//! In a confederation (RFC 5065) the member autonomous systems are linked by confederation
//! eBGP: the member AS goes into the AS_CONFED_SEQUENCE, the next hop and local preference are
//! kept, and the segment is stripped (the confederation identifier takes its place) where the path
//! leaves the confederation.
//! A path is usable only while its next hop resolves in the routing table.
//!
//! Policy: a neighbour's `prefix-list`, `filter-list` and `route-map` (inbound and outbound) and
//! the route map of a `redistribute` are applied where paths are exported, imported and
//! originated; a route map can match communities and AS paths and set communities.
//!
//! Communities (RFC 1997) ride in the UPDATE only when the neighbour has `send-community`; the
//! well-known ones are honoured when a path is advertised (`no-advertise` goes to nobody,
//! `no-export` and `local-AS` to no external neighbour). An `aggregate-address` makes a path of
//! its own while a more specific path exists in the BGP table, and `summary-only` keeps the more
//! specific ones from being advertised. `maximum-paths N` puts up to N equal external paths in the
//! routing table. Neighbours may have their own timers: the hold time is the smaller of the two
//! sides, and a session whose path is lost (without an administrative change or a directly
//! connected interface going down) stays up until its hold time runs out.
//!
//! Also here: `neighbor default-originate`, `remove-private-as`, `soft-reconfiguration inbound`
//! (what the neighbour sent before the inbound policy), `bgp always-compare-med`,
//! `bgp bestpath as-path ignore`, `maximum-paths ibgp` and the discard route an aggregate puts in
//! the routing table.
//!
//! Left out on purpose (see `docs/gaps.md`): AS_CONFED_SET paths (nothing here makes one), route
//! tags, and IPv6 address families.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::bgp::{self as conf, Source};
use netlabcraft_model::policy::{self, Sets, Subject};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::Ipv4;
use netlabcraft_wire::bgp as msg;
use netlabcraft_wire::tcp::{self, Tcp};

use crate::{L3If, RouteEntry, Sim, up_ifaces};

/// Why a session whose two ends have different MD5 passwords stays down.
pub const AUTH_FAILED: &str = "TCP MD5 authentication failed: the password differs or is missing at one end";

/// Administrative distance of a path learned from an external neighbour.
pub const DISTANCE_EBGP: u8 = 20;
/// Administrative distance of a path learned from an internal neighbour.
pub const DISTANCE_IBGP: u8 = 200;
/// The weight of a path this router originates.
pub const WEIGHT_LOCAL: u32 = 32_768;
/// How long a session takes to go from a first connection attempt to Established.
pub const SETUP_US: u64 = 3_000_000;
/// Keepalives are due on the whole minute (the default interval) ...
pub const KEEPALIVE_EVERY_US: u64 = 60_000_000;
/// ... and on every second when some neighbour has its own timers.
pub const KEEPALIVE_TICK_US: u64 = 1_000_000;
/// Rounds of advertisements at most.
const MAX_ROUNDS: usize = 64;
/// Most paths a router keeps from its neighbours, and most it originates.
const MAX_PATHS: usize = 4_096;
const MAX_LOCAL: usize = 1_024;
/// Most routers a TCP connection is followed through.
const MAX_HOPS: u8 = 16;
/// Most AS numbers in the AS set of an aggregate.
const MAX_AS_SET: usize = 32;
/// Most member autonomous systems kept in a confederation path.
const MAX_CONFED_PATH: usize = 32;
/// Most prefixes in one UPDATE.
const PER_UPDATE: usize = 200;
/// The TCP port the side that opened the connection uses.
const CLIENT_PORT: u16 = 40_179;

/// A session as one router sees it: the router and the neighbour address it configured.
pub type SessKey = (DeviceId, Ipv4Addr);

/// Where a session is on its way up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Connect,
    Active,
    OpenSent,
    OpenConfirm,
    Established,
}

impl SessionState {
    pub fn name(self) -> &'static str {
        match self {
            SessionState::Idle => "Idle",
            SessionState::Connect => "Connect",
            SessionState::Active => "Active",
            SessionState::OpenSent => "OpenSent",
            SessionState::OpenConfirm => "OpenConfirm",
            SessionState::Established => "Established",
        }
    }
}

/// One side of a session that can come up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peering {
    pub peer: DeviceId,
    /// The neighbour's address, as this router configured it.
    pub peer_addr: Ipv4Addr,
    /// This router's address on the session: what the neighbour configured for it.
    pub local: Ipv4Addr,
    /// This router's first hop towards the neighbour.
    pub out_iface: String,
    pub next_hop: Ipv4Addr,
    pub ebgp: bool,
    /// Both routers are members of one confederation and in different member autonomous systems
    /// (`ebgp` is also true for such a session: it is external to the member AS only).
    pub confed: bool,
    /// The TTL of what this router sends.
    pub ttl: u8,
}

/// What the lab allows, before any time has passed: the sessions that can come up, why the
/// others cannot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Analysis {
    pub peerings: BTreeMap<SessKey, Peering>,
    /// A route to the neighbour's address exists.
    pub routed: BTreeSet<SessKey>,
    /// The connection works but the OPEN messages are refused.
    pub errors: BTreeMap<SessKey, String>,
    /// The connection is refused before any OPEN: the two ends do not have the same `password`.
    pub auth_failed: BTreeSet<SessKey>,
}

/// A path to a prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub net: Ipv4Net,
    pub next_hop: Ipv4Addr,
    pub as_path: Vec<u32>,
    pub origin: u8,
    pub med: u32,
    pub local_pref: u32,
    pub weight: u32,
    /// The neighbour (as configured here) the path came from; `None` for a local one.
    pub from: Option<Ipv4Addr>,
    /// That neighbour's router id.
    pub from_id: Ipv4Addr,
    /// Learned from an external neighbour.
    pub ebgp: bool,
    /// The next hop resolves in the routing table.
    pub valid: bool,
    pub best: bool,
    /// The interface and next hop the next hop resolves to.
    pub resolved: Option<(String, Ipv4Addr)>,
    /// The communities the path carries.
    pub communities: Vec<u32>,
    /// The AS set of an aggregate made with `as-set`.
    pub as_set: Vec<u32>,
    /// The metric of the route that resolves the next hop (0 for a connected one).
    pub igp: u32,
    /// One of the equal paths `maximum-paths` puts in the routing table besides the best.
    pub multi: bool,
    /// An aggregate with `summary-only` keeps this path from being advertised.
    pub suppressed: bool,
    /// ORIGINATOR_ID: the router id that first told this AS about the path (a reflected path).
    pub originator_id: Option<Ipv4Addr>,
    /// CLUSTER_LIST: the clusters a reflected path went through, the latest first.
    pub cluster_list: Vec<Ipv4Addr>,
    /// The member autonomous systems of the confederation the path crossed, nearest first.
    pub confed_path: Vec<u32>,
    /// Learned from a neighbour in another member AS of the confederation.
    pub confed: bool,
    /// Made by an `aggregate-address`.
    pub aggregate: bool,
}

impl Path {
    /// Learned from an internal neighbour (same AS, or same member AS of a confederation).
    pub fn internal(&self) -> bool {
        self.from.is_some() && !self.ebgp && !self.confed
    }

    /// The AS path length best-path selection compares: an AS set counts as one.
    pub fn path_len(&self) -> usize {
        self.as_path.len() + usize::from(!self.as_set.is_empty())
    }
}

/// A route BGP gives the routing table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BgpRoute {
    pub net: Ipv4Net,
    /// The BGP next hop (what `show ip route` names).
    pub next_hop: Ipv4Addr,
    /// Where the packets go: the next hop's next hop when it is not on a connected network.
    pub via: Ipv4Addr,
    pub iface: String,
    pub distance: u8,
    pub metric: u32,
}

/// A neighbour of a router, as `show ip bgp neighbors` and `summary` list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborView {
    pub addr: Ipv4Addr,
    pub remote_as: Option<u32>,
    pub state: SessionState,
    pub admin_down: bool,
    pub peer: Option<DeviceId>,
    pub peer_router_id: Option<Ipv4Addr>,
    pub local: Option<Ipv4Addr>,
    pub ebgp: bool,
    /// The session joins two member autonomous systems of a confederation.
    pub confed: bool,
    pub error: Option<String>,
    pub received: usize,
    pub sent: usize,
    /// The keepalive interval and hold time in force (negotiated when a session exists).
    pub keepalive_secs: u16,
    pub hold_secs: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterView {
    pub asn: u32,
    pub router_id: Ipv4Addr,
    pub neighbors: Vec<NeighborView>,
    /// Every path, a prefix after another, the best one first.
    pub paths: Vec<Path>,
    pub routes: Vec<BgpRoute>,
    /// What this router advertises to each Established neighbour.
    pub adv: BTreeMap<Ipv4Addr, Vec<(Ipv4Net, msg::PathAttrs)>>,
    /// What each neighbour with `soft-reconfiguration inbound` sent, before the inbound policy.
    pub received: BTreeMap<Ipv4Addr, Vec<(Ipv4Net, msg::PathAttrs)>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BgpState {
    pub routers: BTreeMap<DeviceId, RouterView>,
}

/// Messages a session has carried, by kind (OPEN, UPDATE, NOTIFICATION, KEEPALIVE).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub sent: [u32; 4],
    pub rcvd: [u32; 4],
    pub bytes_sent: u32,
}

fn kind_index(m: &msg::Message) -> usize {
    match m {
        msg::Message::Open(_) => 0,
        msg::Message::Update(_) => 1,
        msg::Message::Notification(_) => 2,
        msg::Message::Keepalive => 3,
    }
}

fn bump(counts: &mut [u32; 4], i: usize) {
    if let Some(x) = counts.get_mut(i) {
        *x = x.saturating_add(1);
    }
}

impl Counters {
    pub fn total_sent(&self) -> u32 {
        self.sent.iter().fold(0u32, |a, b| a.saturating_add(*b))
    }

    pub fn total_rcvd(&self) -> u32 {
        self.rcvd.iter().fold(0u32, |a, b| a.saturating_add(*b))
    }
}

/// What the simulator keeps between looks.
#[derive(Debug, Clone, Default)]
pub struct Runtime {
    pub for_fp: u64,
    pub state: Option<BgpState>,
    pub base: BTreeMap<DeviceId, Vec<RouteEntry>>,
    pub analysis: Analysis,
    /// When each session that can come up first could (µs).
    pub up_at: BTreeMap<SessKey, u64>,
    /// The sessions past their setup time.
    pub settled: BTreeSet<SessKey>,
    /// Established sessions: since when (µs).
    pub since: BTreeMap<SessKey, u64>,
    pub counters: BTreeMap<SessKey, Counters>,
    /// What each Established session was last told.
    pub adv_sent: BTreeMap<SessKey, Vec<(Ipv4Net, msg::PathAttrs)>>,
    /// Established sessions whose path is lost, waiting for the hold time: when it runs out (µs)
    /// and the peering as it was.
    pub held: BTreeMap<SessKey, (u64, Peering)>,
    /// Why a session went down when it was not the lab that said so.
    pub down_why: BTreeMap<SessKey, &'static str>,
}

/// What `show ip bgp …` reads: states refined by the time spent, counters and uptimes.
#[derive(Debug, Clone, Default)]
pub struct BgpView {
    pub routers: BTreeMap<DeviceId, RouterView>,
    pub counters: BTreeMap<SessKey, Counters>,
    pub since: BTreeMap<SessKey, u64>,
    pub now: u64,
}

/// The router id: configured, else the highest loopback or interface address.
pub fn router_id(d: &Device) -> Ipv4Addr {
    d.bgp.as_ref().and_then(|b| b.router_id).or_else(|| crate::loopback::default_router_id(d)).unwrap_or(Ipv4Addr::UNSPECIFIED)
}

/// Where a datagram to `ip` goes out of a router, from its routing table without BGP's routes:
/// the interface and the address to send to.
fn resolve(base: &[RouteEntry], ifs: &[L3If], ip: Ipv4Addr) -> Option<(String, Ipv4Addr)> {
    resolve_metric(base, ifs, ip).map(|(i, v, _)| (i, v))
}

/// [`resolve`] with the metric of the route it went through (0 for a connected network): the IGP
/// metric to a next hop.
fn resolve_metric(base: &[RouteEntry], ifs: &[L3If], ip: Ipv4Addr) -> Option<(String, Ipv4Addr, u32)> {
    if ifs.iter().any(|i| i.net.addr == ip) {
        return None;
    }
    let r = base
        .iter()
        .filter(|r| r.code != 'L' && r.net.contains(ip) && r.iface.as_deref() != Some(crate::eigrp::NULL0))
        .max_by_key(|r| (r.net.prefix, Reverse(r.distance)))?;
    let metric = if r.code == 'C' { 0 } else { r.metric };
    match (r.code, r.via, &r.iface) {
        ('C', _, Some(i)) => Some((i.clone(), ip, metric)),
        (_, Some(v), Some(i)) => Some((i.clone(), v, metric)),
        (_, Some(v), None) => ifs.iter().find(|i| i.net.contains(v)).map(|i| (i.name.clone(), v, metric)),
        (_, None, Some(i)) => Some((i.clone(), ip, metric)),
        _ => None,
    }
}

/// How a datagram gets to the router that owns an address.
#[derive(Debug, Clone)]
struct Hop {
    owner: DeviceId,
    /// Routers crossed, the last included (a neighbour on the same segment is 1).
    hops: u8,
    out_iface: String,
    next_hop: Ipv4Addr,
    /// The address of the interface it leaves by.
    egress: Ipv4Addr,
}

/// Follows a datagram from a router to the owner of `dst` through the routing tables.
fn path_to(t: &Topology, base: &BTreeMap<DeviceId, Vec<RouteEntry>>, from: &Device, dst: Ipv4Addr) -> Option<Hop> {
    let mut cur = from;
    let mut first: Option<(String, Ipv4Addr, Ipv4Addr)> = None;
    for hops in 0..MAX_HOPS {
        let ifs = up_ifaces(t, cur);
        if ifs.iter().any(|i| i.net.addr == dst) {
            let (out_iface, next_hop, egress) = first?;
            return Some(Hop { owner: cur.id, hops, out_iface, next_hop, egress });
        }
        if hops > 0 && !cur.routes_ip() {
            return None;
        }
        let table = base.get(&cur.id)?;
        let (name, nh) = resolve(table, &ifs, dst)?;
        let out = ifs.iter().find(|i| i.name == name)?;
        if first.is_none() {
            first = Some((name.clone(), nh, out.net.addr));
        }
        let reach = crate::ospf::iface_reach(t, cur, out);
        cur = t
            .devices
            .iter()
            .find(|b| b.id != cur.id && up_ifaces(t, b).iter().any(|ib| ib.net.addr == nh && crate::ospf::reaches(t, &reach, b, ib)))?;
    }
    None
}

enum Attempt {
    No,
    /// The connection works but the OPEN messages are refused: the other end's key, and whether
    /// this end is the one that refuses.
    BadAs(SessKey, bool),
    /// The two ends' MD5 passwords differ (or only one end has one): the other end's key.
    BadAuth(SessKey),
    Ok(Box<Peering>, Box<Peering>, SessKey),
}

fn multihop(n: &conf::Neighbor) -> u8 {
    n.ebgp_multihop.unwrap_or(1).max(1)
}

fn attempt(t: &Topology, base: &BTreeMap<DeviceId, Vec<RouteEntry>>, a: &Device, ab: &conf::Bgp, n: &conf::Neighbor, to: &Hop) -> Attempt {
    let Some(b) = t.device(to.owner).filter(|b| b.routes_ip()) else { return Attempt::No };
    let Some(bb) = &b.bgp else { return Attempt::No };
    let ifs = up_ifaces(t, a);
    let src = match &n.update_source {
        Some(name) => match ifs.iter().find(|i| i.name.eq_ignore_ascii_case(name)) {
            Some(i) => i.net.addr,
            None => return Attempt::No,
        },
        None => to.egress,
    };
    let Some(m) = bb.neighbor(src).filter(|m| !m.shutdown && m.remote_as.is_some()) else { return Attempt::No };
    let Some(back) = path_to(t, base, b, src).filter(|h| h.owner == a.id) else { return Attempt::No };
    let bkey = (b.id, m.addr);
    // TCP MD5 (RFC 2385): a segment that does not carry the right signature is dropped, so the
    // connection never opens unless both ends have the same password or neither has one.
    if n.password != m.password {
        return Attempt::BadAuth(bkey);
    }
    // Inside a confederation each side names the other's member AS; from outside, a confederation is
    // known by its identifier.
    let same_confed = ab.confed_id.is_some() && ab.confed_id == bb.confed_id;
    let (their_as, my_as) = if same_confed { (bb.asn, ab.asn) } else { (bb.outer_as(), ab.outer_as()) };
    if n.remote_as != Some(their_as) || m.remote_as != Some(my_as) {
        return Attempt::BadAs(bkey, n.remote_as != Some(their_as));
    }
    let confed = same_confed && ab.asn != bb.asn;
    if confed && !(ab.confed_peers.contains(&bb.asn) && bb.confed_peers.contains(&ab.asn)) {
        return Attempt::No;
    }
    let ebgp = if same_confed { confed } else { ab.outer_as() != bb.outer_as() };
    if !same_confed && !ebgp && (ab.confed_id.is_some() || bb.confed_id.is_some()) {
        return Attempt::No;
    }
    if ebgp {
        let near = |d: &Device, addr: Ipv4Addr| up_ifaces(t, d).iter().any(|i| !crate::loopback::is_loopback(i.port) && i.net.contains(addr));
        if to.hops > multihop(n) || (multihop(n) == 1 && !near(a, n.addr)) {
            return Attempt::No;
        }
        if back.hops > multihop(m) || (multihop(m) == 1 && !near(b, src)) {
            return Attempt::No;
        }
    }
    let ttl = |h: u8| if ebgp { h } else { 255 };
    let mine = Peering {
        peer: b.id,
        peer_addr: n.addr,
        local: src,
        out_iface: to.out_iface.clone(),
        next_hop: to.next_hop,
        ebgp,
        confed,
        ttl: ttl(multihop(n)),
    };
    let theirs = Peering {
        peer: a.id,
        peer_addr: src,
        local: n.addr,
        out_iface: back.out_iface.clone(),
        next_hop: back.next_hop,
        ebgp,
        confed,
        ttl: ttl(multihop(m)),
    };
    Attempt::Ok(Box::new(mine), Box::new(theirs), bkey)
}

/// Which sessions can come up in the lab as it is, given every router's routing table without
/// BGP's routes.
pub fn analyse(t: &Topology, base: &BTreeMap<DeviceId, Vec<RouteEntry>>) -> Analysis {
    let mut an = Analysis::default();
    for a in t.devices.iter().filter(|d| d.routes_ip()) {
        let Some(ab) = &a.bgp else { continue };
        for n in &ab.neighbors {
            if n.shutdown || n.remote_as.is_none() {
                continue;
            }
            let key = (a.id, n.addr);
            let Some(to) = path_to(t, base, a, n.addr) else { continue };
            an.routed.insert(key);
            match attempt(t, base, a, ab, n, &to) {
                Attempt::Ok(mine, theirs, bkey) => {
                    an.peerings.entry(key).or_insert(*mine);
                    an.peerings.entry(bkey).or_insert(*theirs);
                }
                Attempt::BadAs(bkey, mine_refuses) => {
                    let (here, there) = if mine_refuses { ("sent", "received") } else { ("received", "sent") };
                    let why = msg::notification_text(msg::ERR_OPEN, msg::OPEN_BAD_PEER_AS);
                    an.errors.entry(key).or_insert_with(|| format!("{here} NOTIFICATION: {why}"));
                    an.errors.entry(bkey).or_insert_with(|| format!("{there} NOTIFICATION: {why}"));
                }
                Attempt::BadAuth(bkey) => {
                    an.auth_failed.insert(key);
                    an.auth_failed.insert(bkey);
                }
                Attempt::No => {}
            }
        }
    }
    an
}

/// What a router's settings change in the choice of the best path and in how many paths share the
/// load.
#[derive(Debug, Clone, Copy, Default)]
struct Rank {
    /// `maximum-paths N`: equal external paths in the routing table.
    multi: usize,
    /// `maximum-paths ibgp N`: equal internal paths in the routing table.
    multi_ibgp: usize,
    /// `bgp always-compare-med`.
    always_med: bool,
    /// `bgp bestpath as-path ignore`.
    ignore_len: bool,
}

fn rank_of(ab: &conf::Bgp) -> Rank {
    Rank { multi: ab.multipath(), multi_ibgp: ab.multipath_ibgp(), always_med: ab.always_compare_med, ignore_len: ab.as_path_ignore }
}

/// `a` is a better path than `b` to the same prefix.
fn better(a: &Path, b: &Path, o: &Rank) -> bool {
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
    // The MED only compares paths that come from the same neighbouring AS (unless told to always).
    if (o.always_med || a.as_path.first() == b.as_path.first()) && a.med != b.med {
        return a.med < b.med;
    }
    if a.ebgp != b.ebgp {
        return a.ebgp;
    }
    // The nearer next hop by the IGP's metric.
    if a.igp != b.igp {
        return a.igp < b.igp;
    }
    // A reflected path is known by the router that first told the AS about it (RFC 4456), and the
    // one that went through fewer clusters wins a tie after that.
    let (ra, rb) = (a.originator_id.unwrap_or(a.from_id), b.originator_id.unwrap_or(b.from_id));
    if ra != rb {
        return ra < rb;
    }
    if a.cluster_list.len() != b.cluster_list.len() {
        return a.cluster_list.len() < b.cluster_list.len();
    }
    a.from < b.from
}

/// Whether `b` is as good as the best path `a` in every step before the router id and comes from
/// the same neighbouring AS, both being external or both internal: the two can share the load under
/// `maximum-paths` (`maximum-paths ibgp` for internal ones).
fn equal_cost(a: &Path, b: &Path, o: &Rank) -> bool {
    a.valid
        && b.valid
        && a.from.is_some()
        && b.from.is_some()
        && !a.confed
        && !b.confed
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
/// up to `multi - 1` more external paths (`multi_ibgp - 1` internal ones, when the best is internal)
/// as good as the best, through a different first hop, are marked to share the load.
fn rank(cands: Vec<Path>, o: &Rank) -> Vec<Path> {
    let mut groups: BTreeMap<Ipv4Net, Vec<Path>> = BTreeMap::new();
    for c in cands {
        groups.entry(c.net).or_default().push(c);
    }
    let mut out = Vec::new();
    for mut g in groups.into_values() {
        let best = g.iter().enumerate().fold(0usize, |b, (i, c)| match g.get(b) {
            Some(x) if !better(c, x, o) => b,
            _ => i,
        });
        if best < g.len() {
            g.swap(0, best);
        }
        if let Some(p) = g.first_mut() {
            p.best = p.valid;
        }
        if let Some(top) = g.first().filter(|p| p.best).cloned() {
            let multipath = if top.ebgp {
                o.multi
            } else if top.internal() {
                o.multi_ibgp
            } else {
                1
            };
            let mut used: Vec<Option<(String, Ipv4Addr)>> = vec![top.resolved.clone()];
            for p in g.iter_mut().skip(1) {
                if multipath > 1 && used.len() < multipath && equal_cost(&top, p, o) && !used.contains(&p.resolved) {
                    p.multi = true;
                    used.push(p.resolved.clone());
                }
            }
        }
        out.extend(g);
    }
    out
}

/// The paths a router starts from: its `network` prefixes that are in its routing table, and what
/// it redistributes.
fn originate(d: &Device, base: &[RouteEntry]) -> Vec<Path> {
    let Some(b) = &d.bgp else { return Vec::new() };
    let mut nets: Vec<(Ipv4Net, u8, Sets)> = Vec::new();
    for n in &b.networks {
        if base.iter().any(|r| r.code != 'L' && r.net == *n && r.iface.as_deref() != Some(crate::eigrp::NULL0)) && !nets.iter().any(|(x, ..)| x == n)
        {
            nets.push((*n, msg::ORIGIN_IGP, Sets::default()));
        }
    }
    for s in &b.redistribute {
        let codes: &[char] = match s {
            Source::Connected => &['C'],
            Source::Static => &['S'],
            Source::Ospf => &['O', 'I', 'E', 'F', 'N', 'M'],
        };
        let map = b.redistribute_maps.get(s.word()).map(String::as_str);
        for r in base.iter().filter(|r| codes.contains(&r.code)) {
            if nets.iter().any(|(x, ..)| *x == r.net) {
                continue;
            }
            if let Some(sets) = policy::apply(d, None, map, &Subject { net: r.net, metric: r.metric }) {
                nets.push((r.net, msg::ORIGIN_INCOMPLETE, sets));
            }
        }
    }
    nets.truncate(MAX_LOCAL);
    nets.into_iter()
        .map(|(net, origin, sets)| Path {
            net,
            next_hop: sets.next_hop.unwrap_or(Ipv4Addr::UNSPECIFIED),
            as_path: sets.prepend.clone(),
            origin: sets.origin.unwrap_or(origin),
            med: sets.metric.unwrap_or(0),
            local_pref: sets.local_pref.unwrap_or_else(|| b.local_pref()),
            weight: sets.weight.unwrap_or(WEIGHT_LOCAL),
            from: None,
            from_id: Ipv4Addr::UNSPECIFIED,
            ebgp: false,
            valid: true,
            best: false,
            resolved: None,
            communities: policy::merge_communities(&sets, &[]),
            as_set: Vec::new(),
            igp: 0,
            multi: false,
            suppressed: false,
            originator_id: None,
            cluster_list: Vec::new(),
            confed_path: Vec::new(),
            confed: false,
            aggregate: false,
        })
        .collect()
}

/// The paths an `aggregate-address` makes: one per statement that has a more specific usable path
/// under it (and no path of its own for the same prefix from a `network` statement).
fn aggregate_paths(ab: &conf::Bgp, paths: &[Path]) -> Vec<Path> {
    let mut out = Vec::new();
    for g in &ab.aggregates {
        let under = |p: &&Path| p.valid && p.net.prefix > g.net.prefix && g.net.contains(p.net.addr);
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
        out.push(Path {
            net: g.net,
            next_hop: Ipv4Addr::UNSPECIFIED,
            as_path: Vec::new(),
            origin: msg::ORIGIN_IGP,
            med: 0,
            local_pref: ab.local_pref(),
            weight: WEIGHT_LOCAL,
            from: None,
            from_id: Ipv4Addr::UNSPECIFIED,
            ebgp: false,
            valid: true,
            best: false,
            resolved: None,
            communities: Vec::new(),
            as_set: set,
            igp: 0,
            multi: false,
            suppressed: false,
            originator_id: None,
            cluster_list: Vec::new(),
            confed_path: Vec::new(),
            confed: false,
            aggregate: true,
        });
    }
    out
}

/// A `summary-only` aggregate covers this prefix with a shorter one.
fn summarised(ab: &conf::Bgp, net: Ipv4Net) -> bool {
    ab.aggregates.iter().any(|g| g.summary_only && g.net.prefix < net.prefix && g.net.contains(net.addr))
}

/// The cluster a route reflector serves: its `bgp cluster-id`, else its router id.
fn cluster_of(d: &Device, b: &conf::Bgp) -> Ipv4Addr {
    b.cluster_id.unwrap_or_else(|| router_id(d))
}

/// `remove-private-as` on a path sent to an external neighbour: the private AS numbers go, but
/// without `all` only when the whole path is private.
fn strip_private(path: Vec<u32>, n: &conf::Neighbor) -> Vec<u32> {
    if !n.remove_private_as {
        return path;
    }
    if n.remove_private_all {
        return path.into_iter().filter(|a| !conf::is_private_as(*a)).collect();
    }
    if path.iter().all(|a| conf::is_private_as(*a)) { Vec::new() } else { path }
}

/// What a router tells a neighbour about one of its best paths, if it tells it at all: the
/// neighbour's outbound prefix list and route map decide, and the map's `set` lines change what
/// is told (the weight stays here: it is never sent).
fn export(a: &Device, ab: &conf::Bgp, n: &conf::Neighbor, p: &Peering, path: &Path) -> Option<msg::PathAttrs> {
    // Never back to where it came from.
    if path.from == Some(n.addr) {
        return None;
    }
    // A session that leaves the confederation (or the AS, outside any confederation).
    let external = p.ebgp && !p.confed;
    // A path that already crossed the neighbour's AS would only be dropped there.
    if external && n.remote_as.is_some_and(|r| path.as_path.contains(&r) || path.as_set.contains(&r)) {
        return None;
    }
    if p.confed && n.remote_as.is_some_and(|r| path.confed_path.contains(&r)) {
        return None;
    }
    // Well-known communities: no-advertise goes to nobody, no-export stays inside the AS (or the
    // confederation), local-AS inside the member AS.
    if path.communities.contains(&policy::NO_ADVERTISE) {
        return None;
    }
    if external && path.communities.contains(&policy::NO_EXPORT) {
        return None;
    }
    if p.ebgp && path.communities.contains(&policy::LOCAL_AS) {
        return None;
    }
    // A summary-only aggregate covers this prefix.
    if summarised(ab, path.net) {
        return None;
    }
    if let Some(l) = n.filter_list_out.as_deref().and_then(|x| a.as_path_list(x))
        && !l.permits(&path.as_path, &path.as_set)
    {
        return None;
    }
    // Between internal neighbours a path learned from an internal neighbour is not passed on, unless
    // this router is a route reflector: a path from a client goes to every internal neighbour, one
    // from a non-client to the clients only (RFC 4456).
    let reflect = !p.ebgp && path.internal();
    if reflect {
        let from_client = path.from.and_then(|f| ab.neighbor(f)).is_some_and(|s| s.route_reflector_client);
        if !from_client && !n.route_reflector_client {
            return None;
        }
    }
    let seen = policy::Attrs { communities: &path.communities, as_path: &path.as_path, as_set: &path.as_set, tag: 0 };
    let sets = policy::apply_with(a, n.prefix_list_out.as_deref(), n.route_map_out.as_deref(), &Subject { net: path.net, metric: path.med }, &seen)?;
    // Communities travel only to a neighbour with `send-community`.
    let communities = if n.send_community { policy::merge_communities(&sets, &path.communities) } else { Vec::new() };
    let mut tail = sets.prepend.clone();
    tail.extend(path.as_path.iter().copied());
    let origin = sets.origin.unwrap_or(path.origin);
    // A path this router originates names its own address, unless a map named another next hop.
    let own = if path.from.is_none() && !path.next_hop.is_unspecified() { path.next_hop } else { p.local };
    if external {
        let mut as_path = vec![ab.outer_as()];
        as_path.extend(strip_private(tail, n));
        return Some(msg::PathAttrs {
            origin,
            as_path,
            next_hop: sets.next_hop.unwrap_or(own),
            // The MED goes only as far as the next AS, and only for what this router originates
            // (or what a map gave a metric).
            med: sets.metric.or_else(|| path.from.is_none().then_some(path.med)),
            local_pref: None,
            as_set: path.as_set.clone(),
            communities,
            ..msg::PathAttrs::default()
        });
    }
    // Inside the AS, and between the member autonomous systems of a confederation: the next hop, the
    // MED and the local preference go along; across a confederation link our member AS joins the
    // confederation path and the AS path is left as it is.
    let confed_path = if p.confed {
        let mut v = vec![ab.asn];
        v.extend(path.confed_path.iter().copied());
        v.truncate(MAX_CONFED_PATH);
        v
    } else {
        path.confed_path.clone()
    };
    let (originator_id, cluster_list) = if reflect {
        let mut clusters = vec![cluster_of(a, ab)];
        clusters.extend(path.cluster_list.iter().copied());
        clusters.truncate(msg::MAX_CLUSTERS);
        (Some(path.originator_id.unwrap_or(path.from_id)), clusters)
    } else {
        (None, Vec::new())
    };
    // A reflected path keeps its next hop, `next-hop-self` or not.
    let next_hop = if !reflect && (n.next_hop_self || path.from.is_none()) { own } else { path.next_hop };
    Some(msg::PathAttrs {
        origin,
        as_path: tail,
        next_hop: sets.next_hop.unwrap_or(next_hop),
        med: Some(sets.metric.unwrap_or(path.med)),
        local_pref: Some(sets.local_pref.unwrap_or(path.local_pref)),
        as_set: path.as_set.clone(),
        communities,
        originator_id,
        cluster_list,
        confed_path,
        confed_set: Vec::new(),
    })
}

/// The default route `neighbor … default-originate` sends: this router as the next hop, whether or not
/// it has a default route of its own.
fn default_attrs(ab: &conf::Bgp, p: &Peering) -> msg::PathAttrs {
    let external = p.ebgp && !p.confed;
    msg::PathAttrs {
        origin: msg::ORIGIN_IGP,
        as_path: if external { vec![ab.outer_as()] } else { Vec::new() },
        next_hop: p.local,
        med: if external { None } else { Some(0) },
        local_pref: if external { None } else { Some(ab.local_pref()) },
        confed_path: if p.confed { vec![ab.asn] } else { Vec::new() },
        ..msg::PathAttrs::default()
    }
}

/// The default route `neighbor … default-originate` sends. Without a route map it is always sent.
/// With one, it is sent only while some route this router has (its routing table without BGP's
/// routes, and its best BGP paths) passes the map, and the map's `set` lines change it.
fn default_route(
    a: &Device,
    ab: &conf::Bgp,
    n: &conf::Neighbor,
    p: &Peering,
    table: &[RouteEntry],
    best: &[&Path],
) -> Option<(Ipv4Net, msg::PathAttrs)> {
    let net = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0);
    let mut at = default_attrs(ab, p);
    // A map that is named but not defined lets the default through, as everywhere else.
    if let Some(map) = n.default_originate_map.as_deref().filter(|m| a.route_map(m).is_some()) {
        let routes = table.iter().map(|r| (r.net, r.metric, r.tag)).chain(best.iter().map(|x| (x.net, x.med, 0)));
        let sets = routes.filter(|(net, ..)| net.prefix != 0).find_map(|(net, metric, tag)| {
            policy::apply_with(a, None, Some(map), &Subject { net, metric }, &policy::Attrs { tag, ..policy::Attrs::default() })
        })?;
        let external = p.ebgp && !p.confed;
        if external {
            at.as_path.extend(sets.prepend.iter().copied());
            at.med = sets.metric.or(at.med);
        } else {
            at.as_path = sets.prepend.clone();
            at.med = Some(sets.metric.unwrap_or(0));
            at.local_pref = Some(sets.local_pref.unwrap_or(ab.local_pref()));
        }
        if let Some(h) = sets.next_hop {
            at.next_hop = h;
        }
        if let Some(o) = sets.origin {
            at.origin = o;
        }
        if n.send_community {
            at.communities = policy::merge_communities(&sets, &[]);
        }
    }
    Some((net, at))
}

/// Everything a router tells one neighbour: its best paths that [`export`] lets through, and the
/// default route when the neighbour has `default-originate` (unless a path for 0.0.0.0/0 is sent anyway).
fn export_all(
    a: &Device,
    ab: &conf::Bgp,
    n: &conf::Neighbor,
    p: &Peering,
    table: &[RouteEntry],
    best: &[&Path],
) -> Vec<(Ipv4Net, msg::PathAttrs)> {
    let mut out: Vec<(Ipv4Net, msg::PathAttrs)> = best.iter().filter_map(|x| export(a, ab, n, p, x).map(|at| (x.net, at))).collect();
    if n.default_originate && !out.iter().any(|(net, _)| net.prefix == 0) {
        out.extend(default_route(a, ab, n, p, table, best));
    }
    out
}

/// What a received path must pass before any policy: it must not loop through this router's AS (the
/// confederation identifier from outside, the member AS inside), through this member AS, through this
/// router (ORIGINATOR_ID) or this cluster (CLUSTER_LIST), nor name one of this router's addresses as
/// its next hop.
fn loop_free(b: &Device, bb: &conf::Bgp, p: &Peering, attrs: &msg::PathAttrs, own: &[L3If]) -> bool {
    let outer = bb.outer_as();
    if attrs.as_path.contains(&outer) || attrs.as_set.contains(&outer) || own.iter().any(|i| i.net.addr == attrs.next_hop) {
        return false;
    }
    if attrs.confed_path.contains(&bb.asn) || attrs.confed_set.contains(&bb.asn) {
        return false;
    }
    if !p.ebgp && (attrs.originator_id == Some(router_id(b)) || attrs.cluster_list.contains(&cluster_of(b, bb))) {
        return false;
    }
    true
}

/// What a router makes of what a neighbour told it: `None` when it drops it, by the loop checks
/// or by the neighbour's inbound prefix list and route map.
fn import(b: &Device, m: &conf::Neighbor, p: &Peering, attrs: &msg::PathAttrs, from_id: Ipv4Addr, net: Ipv4Net, own: &[L3If]) -> Option<Path> {
    let bb = b.bgp.as_ref()?;
    if !loop_free(b, bb, p, attrs, own) {
        return None;
    }
    if let Some(l) = m.filter_list_in.as_deref().and_then(|x| b.as_path_list(x))
        && !l.permits(&attrs.as_path, &attrs.as_set)
    {
        return None;
    }
    let external = p.ebgp && !p.confed;
    let med = attrs.med.unwrap_or(0);
    let seen = policy::Attrs { communities: &attrs.communities, as_path: &attrs.as_path, as_set: &attrs.as_set, tag: 0 };
    let sets = policy::apply_with(b, m.prefix_list_in.as_deref(), m.route_map_in.as_deref(), &Subject { net, metric: med }, &seen)?;
    // An external neighbour's local preference is ignored; internal and confederation neighbours carry it.
    let local_pref = sets.local_pref.unwrap_or_else(|| if external { bb.local_pref() } else { attrs.local_pref.unwrap_or_else(|| bb.local_pref()) });
    let mut as_path = sets.prepend.clone();
    as_path.extend(attrs.as_path.iter().copied());
    Some(Path {
        net,
        next_hop: sets.next_hop.unwrap_or(attrs.next_hop),
        as_path,
        origin: sets.origin.unwrap_or(attrs.origin),
        med: sets.metric.unwrap_or(med),
        local_pref,
        weight: sets.weight.unwrap_or_else(|| u32::from(m.weight.unwrap_or(0))),
        from: Some(m.addr),
        from_id,
        ebgp: external,
        valid: false,
        best: false,
        resolved: None,
        communities: policy::merge_communities(&sets, &attrs.communities),
        as_set: attrs.as_set.clone(),
        igp: 0,
        multi: false,
        suppressed: false,
        originator_id: if p.ebgp { None } else { attrs.originator_id },
        cluster_list: if p.ebgp { Vec::new() } else { attrs.cluster_list.clone() },
        confed_path: attrs.confed_path.clone(),
        confed: p.confed,
        aggregate: false,
    })
}

type RibIn = BTreeMap<DeviceId, BTreeMap<(Ipv4Addr, Ipv4Net), Path>>;
/// What neighbours with `soft-reconfiguration inbound` sent, before the inbound policy.
type RibRaw = BTreeMap<DeviceId, BTreeMap<(Ipv4Addr, Ipv4Net), msg::PathAttrs>>;

/// A session that is up: both ends are past their setup time.
fn live<'a>(an: &'a Analysis, settled: &BTreeSet<SessKey>, key: &SessKey) -> Option<&'a Peering> {
    let p = an.peerings.get(key)?;
    (settled.contains(key) && settled.contains(&(p.peer, p.local))).then_some(p)
}

/// Every router's paths and routes, once the sessions in `settled` are Established.
pub fn compute(t: &Topology, base: &BTreeMap<DeviceId, Vec<RouteEntry>>, an: &Analysis, settled: &BTreeSet<SessKey>) -> BgpState {
    let routers: Vec<&Device> = t.devices.iter().filter(|d| d.bgp.is_some() && d.routes_ip()).collect();
    let rids: BTreeMap<DeviceId, Ipv4Addr> = routers.iter().map(|d| (d.id, router_id(d))).collect();
    let ifs: BTreeMap<DeviceId, Vec<L3If>> = routers.iter().map(|d| (d.id, up_ifaces(t, d))).collect();
    let local: BTreeMap<DeviceId, Vec<Path>> = routers.iter().map(|d| (d.id, originate(d, base.get(&d.id).map_or(&[][..], Vec::as_slice)))).collect();
    let candidates = |d: &Device, rib: &RibIn| -> Vec<Path> {
        let table = base.get(&d.id).map_or(&[][..], Vec::as_slice);
        let mine = ifs.get(&d.id).map_or(&[][..], Vec::as_slice);
        let mut v: Vec<Path> = local.get(&d.id).cloned().unwrap_or_default();
        for p in rib.get(&d.id).into_iter().flat_map(|m| m.values()) {
            let mut p = p.clone();
            let hop = resolve_metric(table, mine, p.next_hop);
            p.valid = hop.is_some();
            p.igp = hop.as_ref().map_or(0, |h| h.2);
            p.resolved = hop.map(|h| (h.0, h.1));
            v.push(p);
        }
        let Some(ab) = &d.bgp else { return rank(v, &Rank::default()) };
        let made = aggregate_paths(ab, &v);
        v.extend(made);
        for p in &mut v {
            p.suppressed = summarised(ab, p.net);
        }
        rank(v, &rank_of(ab))
    };
    let mut rib: RibIn = BTreeMap::new();
    let mut raw: RibRaw = BTreeMap::new();
    for _ in 0..MAX_ROUNDS {
        let mut next: RibIn = BTreeMap::new();
        let mut next_raw: RibRaw = BTreeMap::new();
        for a in &routers {
            let Some(ab) = &a.bgp else { continue };
            let best: Vec<Path> = candidates(a, &rib).into_iter().filter(|p| p.best).collect();
            let from_id = rids.get(&a.id).copied().unwrap_or(Ipv4Addr::UNSPECIFIED);
            for n in &ab.neighbors {
                let Some(p) = live(an, settled, &(a.id, n.addr)) else { continue };
                let Some(b) = routers.iter().find(|d| d.id == p.peer) else { continue };
                let Some(bb) = &b.bgp else { continue };
                let Some(m) = bb.neighbor(p.local) else { continue };
                let own = ifs.get(&b.id).map_or(&[][..], Vec::as_slice);
                let best_refs: Vec<&Path> = best.iter().collect();
                for (net, attrs) in export_all(a, ab, n, p, base.get(&a.id).map_or(&[][..], Vec::as_slice), &best_refs) {
                    // What the neighbour sent, before the inbound policy, for `received-routes`.
                    if m.soft_reconfiguration_in && loop_free(b, bb, p, &attrs, own) {
                        let slot = next_raw.entry(b.id).or_default();
                        if slot.len() < MAX_PATHS {
                            slot.insert((m.addr, net), attrs.clone());
                        }
                    }
                    let Some(got) = import(b, m, p, &attrs, from_id, net, own) else { continue };
                    let slot = next.entry(b.id).or_default();
                    if slot.len() < MAX_PATHS {
                        slot.insert((m.addr, net), got);
                    }
                }
            }
        }
        raw = next_raw;
        if next == rib {
            break;
        }
        rib = next;
    }
    let mut st = BgpState::default();
    for d in &routers {
        let Some(ab) = &d.bgp else { continue };
        let paths = candidates(d, &rib);
        let mut routes: Vec<BgpRoute> = paths
            .iter()
            .filter(|p| (p.best || p.multi) && p.from.is_some())
            .filter_map(|p| {
                let (iface, via) = p.resolved.clone()?;
                Some(BgpRoute {
                    net: p.net,
                    next_hop: p.next_hop,
                    via,
                    iface,
                    distance: if p.ebgp { DISTANCE_EBGP } else { DISTANCE_IBGP },
                    metric: p.med,
                })
            })
            .collect();
        // An aggregate leaves a discard route behind: what no more specific route covers is dropped
        // here instead of following a default route back towards the sender.
        routes.extend(paths.iter().filter(|p| p.aggregate && p.best).map(|p| BgpRoute {
            net: p.net,
            next_hop: Ipv4Addr::UNSPECIFIED,
            via: Ipv4Addr::UNSPECIFIED,
            iface: crate::eigrp::NULL0.to_string(),
            distance: DISTANCE_IBGP,
            metric: 0,
        }));
        let best_paths: Vec<&Path> = paths.iter().filter(|x| x.best).collect();
        let mut adv = BTreeMap::new();
        let mut neighbors = Vec::new();
        for n in &ab.neighbors {
            let key = (d.id, n.addr);
            let peering = an.peerings.get(&key);
            let active = live(an, settled, &key);
            let sent = match active {
                Some(p) => {
                    let list = export_all(d, ab, n, p, base.get(&d.id).map_or(&[][..], Vec::as_slice), &best_paths);
                    let count = list.len();
                    adv.insert(n.addr, list);
                    count
                }
                None => 0,
            };
            let theirs =
                peering.and_then(|p| routers.iter().find(|x| x.id == p.peer).and_then(|x| x.bgp.as_ref()).and_then(|bb| bb.neighbor(p.local)));
            let (keepalive_secs, hold_secs) = match theirs {
                Some(m) => conf::negotiate(conf::timers_of(n), conf::timers_of(m)),
                None => conf::timers_of(n),
            };
            let state = if n.shutdown || n.remote_as.is_none() {
                SessionState::Idle
            } else if active.is_some() {
                SessionState::Established
            } else if peering.is_some() {
                SessionState::Connect
            } else if an.errors.contains_key(&key) {
                SessionState::Idle
            } else if an.routed.contains(&key) {
                SessionState::Active
            } else {
                SessionState::Idle
            };
            let error = if n.shutdown {
                None
            } else if n.remote_as.is_none() {
                Some("no remote AS configured".to_string())
            } else if an.auth_failed.contains(&key) {
                Some(AUTH_FAILED.to_string())
            } else {
                an.errors.get(&key).cloned()
            };
            neighbors.push(NeighborView {
                addr: n.addr,
                remote_as: n.remote_as,
                state,
                admin_down: n.shutdown,
                peer: peering.map(|p| p.peer),
                peer_router_id: peering.and_then(|p| rids.get(&p.peer).copied()),
                local: peering.map(|p| p.local),
                ebgp: peering.is_some_and(|p| p.ebgp),
                confed: peering.is_some_and(|p| p.confed),
                error,
                received: rib.get(&d.id).map_or(0, |m| m.keys().filter(|(a, _)| *a == n.addr).count()),
                sent,
                keepalive_secs,
                hold_secs,
            });
        }
        let mut received: BTreeMap<Ipv4Addr, Vec<(Ipv4Net, msg::PathAttrs)>> = BTreeMap::new();
        for ((addr, net), at) in raw.get(&d.id).into_iter().flatten() {
            received.entry(*addr).or_default().push((*net, at.clone()));
        }
        st.routers.insert(
            d.id,
            RouterView { asn: ab.asn, router_id: rids.get(&d.id).copied().unwrap_or(Ipv4Addr::UNSPECIFIED), neighbors, paths, routes, adv, received },
        );
    }
    st
}

/// BGP's routes for a device without a running simulator (the free routing-table function):
/// every session that can come up counts as Established.
pub fn standalone_routes(t: &Topology, d: &Device) -> Vec<BgpRoute> {
    let mut e = if t.devices.iter().any(|x| x.eigrp.is_some()) { crate::eigrp::compute(t).routes } else { BTreeMap::new() };
    let mut o = if t.devices.iter().any(|x| x.ospf.is_some()) { crate::ospf::compute(t).routes } else { BTreeMap::new() };
    let mut r = if t.devices.iter().any(|x| x.rip.is_some()) { crate::rip::compute(t).routes } else { BTreeMap::new() };
    let mut base = BTreeMap::new();
    for x in t.devices.iter().filter(|x| x.routes_ip()) {
        let (e, o, r) = (e.remove(&x.id).unwrap_or_default(), o.remove(&x.id).unwrap_or_default(), r.remove(&x.id).unwrap_or_default());
        base.insert(x.id, crate::routing_table_with(t, x, &e, &o, &r, &[]));
    }
    standalone_all(t, &base).remove(&d.id).unwrap_or_default()
}

/// BGP's routes for every router given their routing tables without BGP, every session that can
/// come up counting as Established.
pub fn standalone_all(t: &Topology, base: &BTreeMap<DeviceId, Vec<RouteEntry>>) -> BTreeMap<DeviceId, Vec<BgpRoute>> {
    let an = analyse(t, base);
    let settled: BTreeSet<SessKey> = an.peerings.keys().copied().collect();
    compute(t, base, &an, &settled).routers.into_iter().map(|(id, v)| (id, v.routes)).collect()
}

/// One end of a session as the lab configures it now: the neighbour statement, when it is still
/// there, not shut down and has a remote AS.
fn end_config(t: &Topology, key: SessKey) -> Option<conf::Neighbor> {
    let n = t.device(key.0)?.bgp.as_ref()?.neighbor(key.1)?;
    (!n.shutdown && n.remote_as.is_some()).then(|| n.clone())
}

impl Sim {
    /// BGP's routes for a device, for its routing table and for forwarding.
    pub(crate) fn bgp_routes(&mut self, t: &Topology, d: &Device) -> Vec<BgpRoute> {
        if d.bgp.is_none() {
            return Vec::new();
        }
        self.bgp_refresh(t);
        self.bgp.state.as_ref().and_then(|s| s.routers.get(&d.id)).map(|r| r.routes.clone()).unwrap_or_default()
    }

    /// Brings BGP up to date at the current simulated time: works out which sessions can come up
    /// when the lab changed, lets them reach Established once their setup time has passed, logs
    /// the changes and puts the messages on the wire.
    pub(crate) fn bgp_refresh(&mut self, t: &Topology) {
        if !t.devices.iter().any(|d| d.bgp.is_some()) {
            if self.bgp.state.is_some() || self.bgp.for_fp != 0 {
                self.bgp = Runtime::default();
            }
            return;
        }
        let now = self.now;
        let f = crate::ospf::fingerprint(t);
        // A session whose hold time has run out is over: each end sends a hold-timer NOTIFICATION
        // (which only arrives if the path still works one way) and forgets it.
        let expired: Vec<SessKey> = self.bgp.held.iter().filter(|(_, (until, _))| *until <= now).map(|(k, _)| *k).collect();
        for k in &expired {
            let n = msg::Message::Notification(msg::Notification { code: msg::ERR_HOLD_TIMER, subcode: 0 });
            self.bgp_send(t, *k, &n, now);
            self.bgp.held.remove(k);
            self.bgp.down_why.insert(*k, "hold timer expired");
        }
        let fresh = self.bgp.state.is_none() || self.bgp.for_fp != f || !expired.is_empty();
        let mut started: Vec<SessKey> = Vec::new();
        if fresh {
            self.bgp.for_fp = f;
            let mut base = BTreeMap::new();
            for d in t.devices.iter().filter(|d| d.routes_ip()) {
                let e = self.eigrp_routes(t, d);
                let o = self.ospf_routes(t, d);
                let r = self.rip_routes(t, d);
                base.insert(d.id, crate::routing_table_with(t, d, &e, &o, &r, &[]));
            }
            let mut an = analyse(t, &base);
            self.bgp_hold_lost(t, &mut an, &expired);
            self.bgp.up_at.retain(|k, _| an.peerings.contains_key(k));
            for k in an.peerings.keys() {
                if !self.bgp.up_at.contains_key(k) {
                    self.bgp.up_at.insert(*k, now);
                    started.push(*k);
                }
            }
            self.bgp.base = base;
            self.bgp.analysis = an;
        }
        let settled: BTreeSet<SessKey> = self.bgp.up_at.iter().filter(|(_, at)| now.saturating_sub(**at) >= SETUP_US).map(|(k, _)| *k).collect();
        if !fresh && settled == self.bgp.settled {
            return;
        }
        self.bgp.settled = settled;
        let new = compute(t, &self.bgp.base, &self.bgp.analysis, &self.bgp.settled);
        self.bgp.state = Some(new.clone());
        for k in started {
            self.bgp_start(t, k);
        }
        self.bgp_transitions(t, &new);
    }

    /// An Established session that no longer has a path keeps going, both ends, until its hold
    /// time runs out, unless something ends it at once: the neighbour was shut down, removed or
    /// changed on either side, the hold time is 0, or a directly connected external neighbour's
    /// interface went down at either end (fast external fall-over). The sessions that wait are put
    /// back into the analysis so their routes stay; one whose path came back stops waiting.
    fn bgp_hold_lost(&mut self, t: &Topology, an: &mut Analysis, expired: &[SessKey]) {
        self.bgp.held.retain(|k, _| !an.peerings.contains_key(k));
        let was_up: Vec<SessKey> = self.bgp.since.keys().copied().collect();
        for k in was_up {
            if an.peerings.contains_key(&k) || self.bgp.held.contains_key(&k) || expired.contains(&k) {
                continue;
            }
            let Some(old) = self.bgp.analysis.peerings.get(&k).cloned() else { continue };
            let peer_key = (old.peer, old.local);
            let Some(old_peer) = self.bgp.analysis.peerings.get(&peer_key).cloned() else { continue };
            let (Some(n), Some(m)) = (end_config(t, k), end_config(t, peer_key)) else { continue };
            let hold = conf::negotiate(conf::timers_of(&n), conf::timers_of(&m)).1;
            let fast = |side: &conf::Neighbor, key: SessKey, p: &Peering| {
                let down = t.device(key.0).is_none_or(|d| !up_ifaces(t, d).iter().any(|i| i.name == p.out_iface));
                p.ebgp && multihop(side) == 1 && down
            };
            if hold == 0 || fast(&n, k, &old) || fast(&m, peer_key, &old_peer) {
                continue;
            }
            let until = self.now.saturating_add(u64::from(hold).saturating_mul(1_000_000));
            self.bgp.held.insert(k, (until, old));
            self.bgp.held.entry(peer_key).or_insert((until, old_peer));
        }
        for (k, (_, p)) in &self.bgp.held {
            an.peerings.entry(*k).or_insert_with(|| p.clone());
        }
    }

    /// When the next session finishes its setup, or runs out of hold time.
    pub(crate) fn bgp_next_expiry(&self) -> Option<u64> {
        let setup = self.bgp.up_at.iter().filter(|(k, _)| !self.bgp.settled.contains(*k)).map(|(_, at)| at.saturating_add(SETUP_US)).min();
        let hold = self.bgp.held.values().map(|(until, _)| *until).min();
        match (setup, hold) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// How often keepalives are due: every second when some neighbour has its own timers, else
    /// every minute.
    pub(crate) fn bgp_tick_us(&self, t: &Topology) -> u64 {
        let custom = t.devices.iter().filter_map(|d| d.bgp.as_ref()).any(|b| b.neighbors.iter().any(|n| n.timers.is_some()));
        if custom { KEEPALIVE_TICK_US } else { KEEPALIVE_EVERY_US }
    }

    /// The keepalive interval a session uses on this side (0: none).
    fn bgp_keepalive_secs(&self, t: &Topology, key: SessKey) -> u16 {
        let Some(p) = self.bgp.analysis.peerings.get(&key) else { return 0 };
        let mine = t.device(key.0).and_then(|d| d.bgp.as_ref()).and_then(|b| b.neighbor(key.1));
        let theirs = t.device(p.peer).and_then(|d| d.bgp.as_ref()).and_then(|b| b.neighbor(p.local));
        match (mine, theirs) {
            (Some(a), Some(b)) => conf::negotiate(conf::timers_of(a), conf::timers_of(b)).0,
            _ => conf::KEEPALIVE_SECS,
        }
    }

    /// A session starts its setup: new counters, and the OPEN and the KEEPALIVE that confirms it.
    fn bgp_start(&mut self, t: &Topology, key: SessKey) {
        self.bgp.counters.insert(key, Counters::default());
        if !self.bgp.analysis.peerings.contains_key(&key) {
            return;
        }
        let Some(d) = t.device(key.0) else { return };
        let hold_secs = d.bgp.as_ref().and_then(|b| b.neighbor(key.1)).map_or(conf::HOLD_SECS, |n| conf::timers_of(n).1);
        let open = msg::Message::Open(msg::Open { asn: d.bgp.as_ref().map_or(0, |x| x.asn), hold_secs, bgp_id: router_id(d) });
        let now = self.now;
        self.bgp_send(t, key, &open, now.saturating_add(1_000_000));
        self.bgp_send(t, key, &msg::Message::Keepalive, now.saturating_add(2_000_000));
    }

    fn bgp_debug(&self, d: DeviceId) -> bool {
        self.state.get(&d).is_some_and(|s| s.debug_bgp)
    }

    /// Logs the sessions that came up or went down, and sends what changed.
    fn bgp_transitions(&mut self, t: &Topology, new: &BgpState) {
        let now = self.now;
        let est: BTreeSet<SessKey> = new
            .routers
            .iter()
            .flat_map(|(id, r)| r.neighbors.iter().filter(|n| n.state == SessionState::Established).map(move |n| (*id, n.addr)))
            .collect();
        let down: Vec<SessKey> = self.bgp.since.keys().filter(|k| !est.contains(*k)).copied().collect();
        let up: Vec<SessKey> = est.iter().filter(|k| !self.bgp.since.contains_key(*k)).copied().collect();
        for key in down {
            self.bgp.since.remove(&key);
            self.bgp.adv_sent.remove(&key);
            let Some(d) = t.device(key.0) else { continue };
            let why = match (self.bgp.down_why.remove(&key), d.bgp.as_ref().and_then(|b| b.neighbor(key.1))) {
                (Some(w), _) => w,
                (None, None) => "neighbor deleted",
                (None, Some(n)) if n.shutdown => "admin. shutdown",
                (None, Some(_)) => "session lost (the neighbour is unreachable or was reconfigured)",
            };
            if d.bgp.as_ref().is_some_and(|b| b.log_neighbor_changes) {
                self.log(key.0, &format!("%BGP-5-ADJCHANGE: neighbor {} Down {why}", key.1));
            }
            if self.bgp_debug(key.0) {
                self.log(key.0, &format!("BGP debug: neighbor {} Established -> Idle ({why})", key.1));
            }
        }
        for key in up {
            self.bgp.since.insert(key, now);
            let Some(d) = t.device(key.0) else { continue };
            if d.bgp.as_ref().is_some_and(|b| b.log_neighbor_changes) {
                self.log(key.0, &format!("%BGP-5-ADJCHANGE: neighbor {} Up", key.1));
            }
            if self.bgp_debug(key.0) {
                self.log(key.0, &format!("BGP debug: neighbor {} OpenConfirm -> Established", key.1));
            }
        }
        // What each Established session is told: everything at first, then the differences.
        for key in est {
            let list = new.routers.get(&key.0).and_then(|r| r.adv.get(&key.1)).cloned().unwrap_or_default();
            self.bgp_send_updates(t, key, &list, now.saturating_add(10_000));
        }
    }

    fn bgp_send_updates(&mut self, t: &Topology, key: SessKey, new: &[(Ipv4Net, msg::PathAttrs)], at: u64) {
        let old = self.bgp.adv_sent.get(&key).cloned().unwrap_or_default();
        let withdrawn: Vec<Ipv4Net> = old.iter().filter(|(n, _)| !new.iter().any(|(m, _)| m == n)).map(|(n, _)| *n).collect();
        let mut groups: Vec<(msg::PathAttrs, Vec<Ipv4Net>)> = Vec::new();
        for (n, a) in new {
            if old.iter().any(|(m, b)| m == n && b == a) {
                continue;
            }
            match groups.iter_mut().find(|(g, _)| g == a) {
                Some((_, v)) => v.push(*n),
                None => groups.push((a.clone(), vec![*n])),
            }
        }
        self.bgp.adv_sent.insert(key, new.to_vec());
        let mut when = at;
        for chunk in withdrawn.chunks(PER_UPDATE).take(8) {
            let m = msg::Message::Update(msg::Update { withdrawn: chunk.to_vec(), attrs: None, nlri: Vec::new() });
            self.bgp_send(t, key, &m, when);
            when = when.saturating_add(100);
        }
        for (a, nets) in groups.into_iter().take(64) {
            for chunk in nets.chunks(PER_UPDATE).take(8) {
                let m = msg::Message::Update(msg::Update { withdrawn: Vec::new(), attrs: Some(a.clone()), nlri: chunk.to_vec() });
                self.bgp_send(t, key, &m, when);
                when = when.saturating_add(100);
            }
        }
    }

    /// Puts one message on the wire as a TCP segment to the neighbour, from this router's side of
    /// the session.
    fn bgp_send(&mut self, t: &Topology, from: SessKey, m: &msg::Message, at: u64) {
        let Some(p) = self.bgp.analysis.peerings.get(&from).cloned() else { return };
        let Some(d) = t.device(from.0) else { return };
        let Some(ifc) = up_ifaces(t, d).into_iter().find(|i| i.name == p.out_iface) else { return };
        let Some(mac) =
            t.devices.iter().filter(|x| x.id != d.id).find_map(|x| up_ifaces(t, x).into_iter().find(|i| i.net.addr == p.next_hop).map(|i| i.mac))
        else {
            return;
        };
        let peer_key = (p.peer, p.local);
        let (src_port, dst_port) = if from < peer_key { (CLIENT_PORT, msg::PORT) } else { (msg::PORT, CLIENT_PORT) };
        let bytes = m.encode();
        let len = u32::try_from(bytes.len()).unwrap_or(0);
        let c = self.bgp.counters.entry(from).or_default();
        let seq = 1u32.wrapping_add(c.bytes_sent);
        bump(&mut c.sent, kind_index(m));
        c.bytes_sent = c.bytes_sent.wrapping_add(len);
        let ack = 1u32.wrapping_add(self.bgp.counters.get(&peer_key).map_or(0, |c| c.bytes_sent));
        let seg = Tcp { src_port, dst_port, seq, ack, flags: tcp::ACK | tcp::PSH, window: 16_384, payload: bytes };
        let ip = Ipv4 {
            id: 0,
            ttl: p.ttl,
            protocol: netlabcraft_wire::PROTO_TCP,
            src: p.local,
            dst: p.peer_addr,
            payload: seg.encode(p.local, p.peer_addr),
        };
        self.queue_l3(t, d, &ifc, ifc.mac, mac, ip.encode(), at);
    }

    /// A keepalive from each Established session.
    pub(crate) fn send_bgp_keepalives(&mut self, t: &Topology, at: u64) {
        if !t.devices.iter().any(|d| d.bgp.is_some()) {
            return;
        }
        self.bgp_refresh(t);
        let keys: Vec<SessKey> = self.bgp.since.keys().copied().collect();
        let secs = at / 1_000_000;
        for k in keys {
            let every = u64::from(self.bgp_keepalive_secs(t, k));
            if every != 0 && secs.is_multiple_of(every) {
                self.bgp_send(t, k, &msg::Message::Keepalive, at);
            }
        }
    }

    /// A BGP segment arrives at a router: counted, reported when `debug ip bgp` is on, and taken.
    /// `None` when the segment is not for BGP here.
    pub(crate) fn bgp_in(&mut self, d: &Device, ip: &Ipv4, seg: &Tcp) -> Option<Result<(), String>> {
        d.bgp.as_ref()?;
        if seg.payload.is_empty() || (seg.dst_port != msg::PORT && seg.src_port != msg::PORT) {
            return None;
        }
        let key = (d.id, ip.src);
        if !self.bgp.analysis.peerings.contains_key(&key) {
            return Some(Err(format!("BGP: no session with {} here", ip.src)));
        }
        let debug = self.bgp_debug(d.id);
        for m in msg::decode_all(&seg.payload) {
            let c = self.bgp.counters.entry(key).or_default();
            bump(&mut c.rcvd, kind_index(&m));
            if debug {
                self.log(d.id, &format!("BGP debug: {} from {}", msg::message_text(&m), ip.src));
            }
        }
        Some(Ok(()))
    }

    /// `clear ip bgp`: the sessions of a router (one neighbour, or all) go down and start again.
    /// Returns how many were Established.
    pub fn bgp_clear(&mut self, t: &Topology, dev: DeviceId, only: Option<Ipv4Addr>) -> usize {
        self.bgp_refresh(t);
        let keys: Vec<SessKey> = self.bgp.analysis.peerings.keys().filter(|k| k.0 == dev && only.is_none_or(|a| a == k.1)).copied().collect();
        let now = self.now;
        let mut cleared = 0usize;
        for key in keys {
            let Some(p) = self.bgp.analysis.peerings.get(&key).cloned() else { continue };
            let peer_key = (p.peer, p.local);
            if self.bgp.since.contains_key(&key) {
                cleared += 1;
                let n = msg::Message::Notification(msg::Notification { code: msg::ERR_CEASE, subcode: msg::CEASE_RESET });
                self.bgp_send(t, key, &n, now);
                for k in [key, peer_key] {
                    if t.device(k.0).and_then(|d| d.bgp.as_ref()).is_some_and(|b| b.log_neighbor_changes) {
                        self.log(k.0, &format!("%BGP-5-ADJCHANGE: neighbor {} Down User reset", k.1));
                    }
                    self.bgp.since.remove(&k);
                    self.bgp.held.remove(&k);
                    self.bgp.adv_sent.remove(&k);
                }
            }
            self.bgp.up_at.insert(key, now);
            self.bgp.up_at.insert(peer_key, now);
            self.bgp.settled.remove(&key);
            self.bgp.settled.remove(&peer_key);
            self.bgp.held.remove(&key);
            self.bgp.held.remove(&peer_key);
            self.bgp_start(t, key);
            self.bgp_start(t, peer_key);
        }
        self.bgp.state = None;
        self.bgp_refresh(t);
        cleared
    }

    /// What `show ip bgp` and its relatives read.
    pub fn bgp_state(&mut self, t: &Topology) -> BgpView {
        self.bgp_refresh(t);
        let now = self.now;
        let mut routers = self.bgp.state.as_ref().map(|s| s.routers.clone()).unwrap_or_default();
        for (id, r) in &mut routers {
            for n in &mut r.neighbors {
                if n.state == SessionState::Connect {
                    let age = self.bgp.up_at.get(&(*id, n.addr)).map_or(0, |at| now.saturating_sub(*at));
                    n.state = if age >= 2_000_000 {
                        SessionState::OpenConfirm
                    } else if age >= 1_000_000 {
                        SessionState::OpenSent
                    } else {
                        SessionState::Connect
                    };
                }
            }
        }
        BgpView { routers, counters: self.bgp.counters.clone(), since: self.bgp.since.clone(), now }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn better_std(a: &Path, b: &Path) -> bool {
        better(a, b, &Rank::default())
    }

    fn rank_n(cands: Vec<Path>, multi: usize) -> Vec<Path> {
        rank(cands, &Rank { multi, ..Rank::default() })
    }

    /// A router that runs BGP in `asn`.
    fn speaker(asn: u32) -> Device {
        let mut t = Topology::new();
        let id = t.add_device(netlabcraft_model::DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let mut d = t.device(id).unwrap().clone();
        d.bgp = Some(conf::Bgp::new(asn));
        d
    }

    fn path(net: &str, as_path: &[u32]) -> Path {
        Path {
            net: net.parse().unwrap(),
            next_hop: Ipv4Addr::new(10, 0, 0, 2),
            as_path: as_path.to_vec(),
            origin: msg::ORIGIN_IGP,
            med: 0,
            local_pref: 100,
            weight: 0,
            from: Some(Ipv4Addr::new(10, 0, 0, 2)),
            from_id: Ipv4Addr::new(2, 2, 2, 2),
            ebgp: true,
            valid: true,
            best: false,
            resolved: Some(("g0/0".into(), Ipv4Addr::new(10, 0, 0, 2))),
            communities: Vec::new(),
            as_set: Vec::new(),
            igp: 0,
            multi: false,
            suppressed: false,
            originator_id: None,
            cluster_list: Vec::new(),
            confed_path: Vec::new(),
            confed: false,
            aggregate: false,
        }
    }

    #[test]
    fn best_path_follows_the_decision_order() {
        let base = path("10.9.0.0/16", &[65_002, 65_009]);
        // Shorter AS path wins when nothing before it differs.
        let short = Path { from: Some(Ipv4Addr::new(10, 0, 0, 6)), as_path: vec![65_003], ..base.clone() };
        assert!(better_std(&short, &base) && !better_std(&base, &short));
        // A higher local preference beats a shorter path.
        let pref = Path { local_pref: 200, ..base.clone() };
        assert!(better_std(&pref, &short));
        // A higher weight beats a higher local preference.
        let heavy = Path { weight: 10, ..base.clone() };
        assert!(better_std(&heavy, &pref));
        // A locally originated path beats one learned, other things equal.
        let local = Path { from: None, ebgp: false, ..base.clone() };
        assert!(better_std(&local, &base));
        // Lower origin, then lower MED (same neighbouring AS), then external, then lowest router id.
        let egp = Path { origin: msg::ORIGIN_EGP, ..base.clone() };
        assert!(better_std(&base, &egp));
        let low_med = Path { med: 5, ..base.clone() };
        let high_med = Path { med: 50, from: Some(Ipv4Addr::new(10, 0, 0, 6)), ..base.clone() };
        assert!(better_std(&low_med, &high_med));
        let internal = Path { ebgp: false, ..base.clone() };
        assert!(better_std(&base, &internal));
        let low_id = Path { from_id: Ipv4Addr::new(1, 1, 1, 1), from: Some(Ipv4Addr::new(10, 0, 0, 6)), ..base.clone() };
        assert!(better_std(&low_id, &base));
        // A path whose next hop does not resolve loses to anything that does.
        let dead = Path { valid: false, weight: 99, ..base.clone() };
        assert!(better_std(&base, &dead));
    }

    #[test]
    fn rank_puts_the_best_path_first_and_marks_only_a_usable_one() {
        let a = path("10.9.0.0/16", &[65_002, 65_009]);
        let b = Path { as_path: vec![65_003], from: Some(Ipv4Addr::new(10, 0, 0, 6)), ..a.clone() };
        let other = path("10.8.0.0/16", &[65_002]);
        let ranked = rank_n(vec![a.clone(), b.clone(), other], 1);
        assert_eq!(ranked.len(), 3);
        let first = ranked.iter().find(|p| p.net == a.net).unwrap();
        assert_eq!(first.as_path, vec![65_003]);
        assert_eq!(ranked.iter().filter(|p| p.best).count(), 2);
        let dead = Path { valid: false, ..a };
        assert!(rank_n(vec![dead], 1).iter().all(|p| !p.best));
    }

    #[test]
    fn exports_follow_the_ibgp_and_ebgp_rules() {
        let a = speaker(65_001);
        let ab = conf::Bgp::new(65_001);
        let n_ext = {
            let mut n = conf::Neighbor::new(Ipv4Addr::new(10, 0, 0, 2));
            n.remote_as = Some(65_002);
            n
        };
        let ext = Peering {
            peer: DeviceId(2),
            peer_addr: n_ext.addr,
            local: Ipv4Addr::new(10, 0, 0, 1),
            out_iface: "g0/0".into(),
            next_hop: n_ext.addr,
            ebgp: true,
            confed: false,
            ttl: 1,
        };
        let mut learned = path("10.9.0.0/16", &[65_005]);
        learned.from = Some(Ipv4Addr::new(10, 0, 9, 9));
        let sent = export(&a, &ab, &n_ext, &ext, &learned).unwrap();
        // External: our AS in front, ourselves as the next hop, no local preference.
        assert_eq!(sent.as_path, vec![65_001, 65_005]);
        assert_eq!(sent.next_hop, ext.local);
        assert_eq!((sent.med, sent.local_pref), (None, None));
        // Never back to the neighbour it came from, nor to an AS already in the path.
        let back = Path { from: Some(n_ext.addr), ..learned.clone() };
        assert!(export(&a, &ab, &n_ext, &ext, &back).is_none());
        let looped = Path { as_path: vec![65_002], ..learned.clone() };
        assert!(export(&a, &ab, &n_ext, &ext, &looped).is_none());
        // Internal: a path learned from an internal neighbour is not passed on; an external one is, unchanged.
        let n_int = conf::Neighbor { remote_as: Some(65_001), ..conf::Neighbor::new(Ipv4Addr::new(10, 0, 5, 2)) };
        let int = Peering { ebgp: false, ttl: 255, local: Ipv4Addr::new(10, 0, 5, 1), peer_addr: n_int.addr, ..ext.clone() };
        let from_internal = Path { ebgp: false, ..learned.clone() };
        assert!(export(&a, &ab, &n_int, &int, &from_internal).is_none());
        let kept = export(&a, &ab, &n_int, &int, &learned).unwrap();
        assert_eq!((kept.as_path.clone(), kept.next_hop, kept.local_pref), (vec![65_005], learned.next_hop, Some(100)));
        // `next-hop-self` and a locally originated path use our own address.
        let nhs = conf::Neighbor { next_hop_self: true, ..n_int.clone() };
        assert_eq!(export(&a, &ab, &nhs, &int, &learned).unwrap().next_hop, int.local);
        let own = Path { from: None, ebgp: false, next_hop: Ipv4Addr::UNSPECIFIED, as_path: Vec::new(), ..learned };
        assert_eq!(export(&a, &ab, &n_int, &int, &own).unwrap().next_hop, int.local);
    }

    #[test]
    fn imports_drop_a_path_through_our_own_as() {
        let b = speaker(65_002);
        let m = conf::Neighbor::new(Ipv4Addr::new(10, 0, 0, 1));
        let p = Peering {
            peer: DeviceId(1),
            peer_addr: m.addr,
            local: Ipv4Addr::new(10, 0, 0, 2),
            out_iface: "g0/0".into(),
            next_hop: m.addr,
            ebgp: true,
            confed: false,
            ttl: 1,
        };
        let attrs = msg::PathAttrs {
            origin: 0,
            as_path: vec![65_001],
            next_hop: m.addr,
            med: None,
            local_pref: Some(300),
            as_set: Vec::new(),
            communities: Vec::new(),
            ..msg::PathAttrs::default()
        };
        let net: Ipv4Net = "10.1.0.0/16".parse().unwrap();
        let got = import(&b, &m, &p, &attrs, Ipv4Addr::new(1, 1, 1, 1), net, &[]).unwrap();
        // The local preference of an external neighbour is ours, whatever it said.
        assert_eq!((got.local_pref, got.weight, got.ebgp), (100, 0, true));
        let looped = msg::PathAttrs { as_path: vec![65_001, 65_002], ..attrs };
        assert!(import(&b, &m, &p, &looped, Ipv4Addr::new(1, 1, 1, 1), net, &[]).is_none());
    }

    #[test]
    fn neighbour_policy_filters_and_changes_what_is_imported_and_exported() {
        use netlabcraft_model::policy::{PrefixEntry, PrefixList, RouteMap};
        let mut d = speaker(65_002);
        let mut list = PrefixList::new("ten");
        list.add(PrefixEntry { seq: 5, permit: true, net: "10.0.0.0/8".parse().unwrap(), ge: None, le: Some(24) }).unwrap();
        d.prefix_lists.push(list);
        let mut map = RouteMap::new("pref");
        let c = map.clause_mut(10, true).unwrap();
        c.sets.local_pref = Some(250);
        c.sets.weight = Some(9);
        c.sets.prepend = vec![65_002, 65_002];
        d.route_maps.push(map);
        let mut m = conf::Neighbor::new(Ipv4Addr::new(10, 0, 0, 1));
        m.prefix_list_in = Some("ten".into());
        m.route_map_in = Some("pref".into());
        let p = Peering {
            peer: DeviceId(1),
            peer_addr: m.addr,
            local: Ipv4Addr::new(10, 0, 0, 2),
            out_iface: "g0/0".into(),
            next_hop: m.addr,
            ebgp: true,
            confed: false,
            ttl: 1,
        };
        let attrs = msg::PathAttrs {
            origin: 0,
            as_path: vec![65_001],
            next_hop: m.addr,
            med: None,
            local_pref: None,
            as_set: Vec::new(),
            communities: Vec::new(),
            ..msg::PathAttrs::default()
        };
        let rid = Ipv4Addr::new(1, 1, 1, 1);
        let inside: Ipv4Net = "10.1.0.0/16".parse().unwrap();
        let outside: Ipv4Net = "192.168.1.0/24".parse().unwrap();
        let got = import(&d, &m, &p, &attrs, rid, inside, &[]).unwrap();
        assert_eq!((got.local_pref, got.weight, got.as_path.clone()), (250, 9, vec![65_002, 65_002, 65_001]));
        assert!(import(&d, &m, &p, &attrs, rid, outside, &[]).is_none(), "the prefix list denies it");
        // Outbound: the list and the map of the neighbour decide what is sent.
        let mut n = conf::Neighbor::new(Ipv4Addr::new(10, 0, 0, 1));
        n.remote_as = Some(65_001);
        n.prefix_list_out = Some("ten".into());
        n.route_map_out = Some("pref".into());
        let ab = conf::Bgp::new(65_002);
        let mut sent = path("10.1.0.0/16", &[65_005]);
        sent.from = Some(Ipv4Addr::new(10, 0, 9, 9));
        let told = export(&d, &ab, &n, &p, &sent).unwrap();
        assert_eq!(told.as_path, vec![65_002, 65_002, 65_002, 65_005]);
        let hidden = Path { net: outside, ..sent };
        assert!(export(&d, &ab, &n, &p, &hidden).is_none());
    }

    fn ext_peering() -> (conf::Neighbor, Peering) {
        let mut n = conf::Neighbor::new(Ipv4Addr::new(10, 0, 0, 2));
        n.remote_as = Some(65_002);
        let p = Peering {
            peer: DeviceId(2),
            peer_addr: n.addr,
            local: Ipv4Addr::new(10, 0, 0, 1),
            out_iface: "g0/0".into(),
            next_hop: n.addr,
            ebgp: true,
            confed: false,
            ttl: 1,
        };
        (n, p)
    }

    #[test]
    fn communities_travel_only_with_send_community_and_well_known_ones_limit_where_a_path_goes() {
        let a = speaker(65_001);
        let ab = conf::Bgp::new(65_001);
        let (mut n, p) = ext_peering();
        let tagged = Path { communities: vec![100, 200], from: Some(Ipv4Addr::new(10, 0, 9, 9)), ..path("10.9.0.0/16", &[65_005]) };
        assert!(export(&a, &ab, &n, &p, &tagged).unwrap().communities.is_empty(), "no send-community: stripped");
        n.send_community = true;
        assert_eq!(export(&a, &ab, &n, &p, &tagged).unwrap().communities, vec![100, 200]);
        // no-export stays inside the AS; no-advertise goes nowhere; local-AS acts like no-export.
        let no_export = Path { communities: vec![policy::NO_EXPORT], ..tagged.clone() };
        assert!(export(&a, &ab, &n, &p, &no_export).is_none());
        let local_as = Path { communities: vec![policy::LOCAL_AS], ..tagged.clone() };
        assert!(export(&a, &ab, &n, &p, &local_as).is_none());
        let n_int = conf::Neighbor { remote_as: Some(65_001), send_community: true, ..conf::Neighbor::new(Ipv4Addr::new(10, 0, 5, 2)) };
        let int = Peering { ebgp: false, ttl: 255, peer_addr: n_int.addr, ..p.clone() };
        assert_eq!(export(&a, &ab, &n_int, &int, &no_export).unwrap().communities, vec![policy::NO_EXPORT]);
        let no_adv = Path { communities: vec![policy::NO_ADVERTISE], ..tagged };
        assert!(export(&a, &ab, &n_int, &int, &no_adv).is_none());
        assert!(export(&a, &ab, &n, &p, &no_adv).is_none());
    }

    #[test]
    fn an_outbound_map_sets_communities_and_an_inbound_map_matches_them() {
        use netlabcraft_model::policy::{CommunityEntry, CommunityList, RouteMap};
        let mut d = speaker(65_002);
        let mut map = RouteMap::new("tag");
        let c = map.clause_mut(10, true).unwrap();
        c.sets.community = Some(vec![(65_002 << 16) | 7]);
        c.sets.community_additive = true;
        d.route_maps.push(map);
        let mut list = CommunityList::new("seven", false);
        list.entries.push(CommunityEntry { permit: true, communities: vec![(65_002 << 16) | 7], regex: String::new() });
        d.community_lists.push(list);
        let mut pick = RouteMap::new("pick");
        let c = pick.clause_mut(10, true).unwrap();
        c.community_lists = vec!["seven".into()];
        c.sets.local_pref = Some(400);
        d.route_maps.push(pick);
        let (mut n, p) = ext_peering();
        n.send_community = true;
        n.route_map_out = Some("tag".into());
        let ab = conf::Bgp::new(65_002);
        let mut held = path("10.9.0.0/16", &[65_005]);
        held.from = Some(Ipv4Addr::new(10, 0, 9, 9));
        held.communities = vec![5];
        let sent = export(&d, &ab, &n, &p, &held).unwrap();
        assert_eq!(sent.communities, vec![5, (65_002 << 16) | 7]);
        // The receiver's inbound map only matches paths that carry the community.
        let mut rx = speaker(65_003);
        rx.route_maps = d.route_maps.clone();
        rx.community_lists = d.community_lists.clone();
        let mut m = conf::Neighbor::new(Ipv4Addr::new(10, 0, 0, 1));
        m.route_map_in = Some("pick".into());
        let q = Peering { peer_addr: m.addr, local: Ipv4Addr::new(10, 0, 0, 2), ..p };
        let net: Ipv4Net = "10.9.0.0/16".parse().unwrap();
        let rid = Ipv4Addr::new(1, 1, 1, 1);
        let got = import(&rx, &m, &q, &sent, rid, net, &[]).unwrap();
        assert_eq!((got.local_pref, got.communities.clone()), (400, sent.communities.clone()));
        let bare = msg::PathAttrs { communities: Vec::new(), ..sent };
        assert!(import(&rx, &m, &q, &bare, rid, net, &[]).is_none(), "the map has no clause for a path without it");
    }

    #[test]
    fn an_aggregate_exists_while_a_more_specific_path_does_and_summary_only_hides_the_rest() {
        let mut ab = conf::Bgp::new(65_001);
        ab.aggregates.push(conf::Aggregate { net: "10.0.0.0/8".parse().unwrap(), summary_only: true, as_set: true });
        let inside = Path { from: Some(Ipv4Addr::new(10, 0, 9, 9)), ..path("10.9.0.0/16", &[65_005, 65_006]) };
        let outside = path("172.16.0.0/16", &[65_007]);
        assert!(aggregate_paths(&ab, std::slice::from_ref(&outside)).is_empty());
        let made = aggregate_paths(&ab, &[inside.clone(), outside]);
        assert_eq!(made.len(), 1);
        assert_eq!((made[0].net, made[0].from, made[0].as_set.clone()), ("10.0.0.0/8".parse().unwrap(), None, vec![65_005, 65_006]));
        assert_eq!(made[0].path_len(), 1, "an AS set counts as one hop");
        // A path that does not resolve makes nothing; a path of the same prefix of our own wins.
        assert!(aggregate_paths(&ab, &[Path { valid: false, ..inside.clone() }]).is_empty());
        let own = Path { net: "10.0.0.0/8".parse().unwrap(), from: None, ..inside.clone() };
        assert!(aggregate_paths(&ab, &[inside.clone(), own]).is_empty());
        // summary-only keeps the covered prefixes (not the aggregate) from being advertised.
        assert!(summarised(&ab, inside.net) && !summarised(&ab, made[0].net));
        let a = speaker(65_001);
        let (n, p) = ext_peering();
        assert!(export(&a, &ab, &n, &p, &inside).is_none());
        let told = export(&a, &ab, &n, &p, &made[0]).unwrap();
        assert_eq!((told.as_path, told.as_set), (vec![65_001], vec![65_005, 65_006]));
        // Without summary-only the specific one is advertised too.
        ab.aggregates[0].summary_only = false;
        assert!(export(&a, &ab, &n, &p, &inside).is_some());
    }

    #[test]
    fn equal_external_paths_share_the_routing_table_up_to_maximum_paths() {
        let a = path("10.9.0.0/16", &[65_002]);
        let via = |last: u8, from: u8| Path {
            from: Some(Ipv4Addr::new(10, 0, last, 2)),
            from_id: Ipv4Addr::new(from, from, from, from),
            next_hop: Ipv4Addr::new(10, 0, last, 2),
            resolved: Some((format!("g0/{last}"), Ipv4Addr::new(10, 0, last, 2))),
            ..a.clone()
        };
        let paths = vec![via(1, 1), via(2, 2), via(3, 3)];
        let one = rank_n(paths.clone(), 1);
        assert_eq!(one.iter().filter(|p| p.multi).count(), 0);
        let two = rank_n(paths.clone(), 2);
        assert_eq!((two.iter().filter(|p| p.best).count(), two.iter().filter(|p| p.multi).count()), (1, 1));
        let all = rank_n(paths.clone(), 8);
        assert_eq!(all.iter().filter(|p| p.multi).count(), 2);
        // A longer path, an internal path or a different MED is not equal.
        let longer = Path { as_path: vec![65_002, 65_009], ..via(4, 4) };
        let internal = Path { ebgp: false, ..via(5, 5) };
        let other_med = Path { med: 9, ..via(6, 6) };
        let other_as = Path { as_path: vec![65_009], ..via(7, 7) };
        let mixed = rank_n(vec![via(1, 1), longer, internal, other_med, other_as], 8);
        assert_eq!(mixed.iter().filter(|p| p.multi).count(), 0);
    }

    #[test]
    fn the_igp_metric_to_the_next_hop_breaks_a_tie_before_the_router_id() {
        let base = path("10.9.0.0/16", &[65_002]);
        let far = Path { igp: 30, from_id: Ipv4Addr::new(1, 1, 1, 1), from: Some(Ipv4Addr::new(10, 0, 8, 8)), ..base.clone() };
        let near = Path { igp: 10, from_id: Ipv4Addr::new(9, 9, 9, 9), from: Some(Ipv4Addr::new(10, 0, 9, 9)), ..base };
        assert!(better_std(&near, &far) && !better_std(&far, &near));
    }

    #[test]
    fn a_route_reflector_passes_internal_paths_by_the_client_rules() {
        let a = speaker(65_001);
        let mut ab = conf::Bgp::new(65_001);
        ab.cluster_id = Some(Ipv4Addr::new(100, 0, 0, 1));
        let (client, other_client, non_client, other_non) =
            (Ipv4Addr::new(10, 0, 5, 2), Ipv4Addr::new(10, 0, 6, 2), Ipv4Addr::new(10, 0, 7, 2), Ipv4Addr::new(10, 0, 8, 2));
        for (addr, is_client) in [(client, true), (other_client, true), (non_client, false), (other_non, false)] {
            if let Some(n) = ab.neighbor_mut(addr) {
                n.remote_as = Some(65_001);
                n.route_reflector_client = is_client;
            }
        }
        let (_, ext) = ext_peering();
        let int = Peering { ebgp: false, ttl: 255, ..ext };
        let from = |addr: Ipv4Addr| Path { from: Some(addr), ebgp: false, from_id: Ipv4Addr::new(7, 7, 7, 7), ..path("10.9.0.0/16", &[65_005]) };
        let to = |addr: Ipv4Addr| ab.neighbor(addr).cloned().unwrap();
        // A path from a client goes to the other clients and to the non-clients, with the originator
        // and this cluster added and the next hop left alone, `next-hop-self` or not.
        let reflected = export(&a, &ab, &to(other_client), &int, &from(client)).unwrap();
        assert_eq!(reflected.originator_id, Some(Ipv4Addr::new(7, 7, 7, 7)));
        assert_eq!(reflected.cluster_list, vec![Ipv4Addr::new(100, 0, 0, 1)]);
        assert_eq!(reflected.next_hop, from(client).next_hop);
        let nhs = conf::Neighbor { next_hop_self: true, ..to(non_client) };
        assert_eq!(export(&a, &ab, &nhs, &int, &from(client)).unwrap().next_hop, from(client).next_hop);
        assert!(export(&a, &ab, &to(non_client), &int, &from(client)).is_some());
        // A path from a non-client goes to the clients only.
        assert!(export(&a, &ab, &to(client), &int, &from(non_client)).is_some());
        assert!(export(&a, &ab, &to(other_non), &int, &from(non_client)).is_none());
        // Never back to the neighbour it came from, even a client.
        assert!(export(&a, &ab, &to(client), &int, &from(client)).is_none());
        // A path that was reflected before keeps its originator and grows the cluster list.
        let again = Path { originator_id: Some(Ipv4Addr::new(4, 4, 4, 4)), cluster_list: vec![Ipv4Addr::new(200, 0, 0, 1)], ..from(client) };
        let out = export(&a, &ab, &to(other_client), &int, &again).unwrap();
        assert_eq!(out.originator_id, Some(Ipv4Addr::new(4, 4, 4, 4)));
        assert_eq!(out.cluster_list, vec![Ipv4Addr::new(100, 0, 0, 1), Ipv4Addr::new(200, 0, 0, 1)]);
        // A path learned from an external neighbour is not reflected, so it carries neither attribute.
        let outside = Path { ebgp: true, ..from(client) };
        let plain = export(&a, &ab, &to(non_client), &int, &outside).unwrap();
        assert_eq!((plain.originator_id, plain.cluster_list.len()), (None, 0));
        // Neither attribute leaves the AS.
        let (n_ext, ext) = ext_peering();
        let told = export(&a, &ab, &n_ext, &ext, &from(client)).unwrap();
        assert_eq!((told.originator_id, told.cluster_list.len()), (None, 0));
    }

    #[test]
    fn a_reflected_path_is_dropped_by_its_originator_and_by_its_cluster() {
        let mut d = speaker(65_001);
        if let Some(b) = d.bgp.as_mut() {
            b.router_id = Some(Ipv4Addr::new(1, 1, 1, 1));
        }
        let m = {
            let mut m = conf::Neighbor::new(Ipv4Addr::new(10, 0, 5, 1));
            m.remote_as = Some(65_001);
            m
        };
        let (_, ext) = ext_peering();
        let int = Peering { ebgp: false, ttl: 255, ..ext };
        let attrs = msg::PathAttrs {
            as_path: vec![65_005],
            next_hop: Ipv4Addr::new(10, 0, 9, 9),
            local_pref: Some(100),
            originator_id: Some(Ipv4Addr::new(3, 3, 3, 3)),
            cluster_list: vec![Ipv4Addr::new(100, 0, 0, 1)],
            ..msg::PathAttrs::default()
        };
        let net: Ipv4Net = "10.9.0.0/16".parse().unwrap();
        let rid = Ipv4Addr::new(2, 2, 2, 2);
        let got = import(&d, &m, &int, &attrs, rid, net, &[]).unwrap();
        assert_eq!((got.originator_id, got.cluster_list.clone()), (attrs.originator_id, attrs.cluster_list.clone()));
        // Our own router id as the originator: the path came from here.
        let mine = msg::PathAttrs { originator_id: Some(Ipv4Addr::new(1, 1, 1, 1)), ..attrs.clone() };
        assert!(import(&d, &m, &int, &mine, rid, net, &[]).is_none());
        // Our cluster id (the router id when none is set, else `bgp cluster-id`) in the list.
        let ours = msg::PathAttrs { cluster_list: vec![Ipv4Addr::new(100, 0, 0, 1), Ipv4Addr::new(1, 1, 1, 1)], ..attrs.clone() };
        assert!(import(&d, &m, &int, &ours, rid, net, &[]).is_none());
        if let Some(b) = d.bgp.as_mut() {
            b.cluster_id = Some(Ipv4Addr::new(100, 0, 0, 1));
        }
        assert!(import(&d, &m, &int, &attrs, rid, net, &[]).is_none(), "the cluster id is the configured one now");
        assert!(import(&d, &m, &int, &ours, rid, net, &[]).is_none());
        // The tie-break uses the originator in place of the router that told us.
        let direct = Path { from_id: Ipv4Addr::new(5, 5, 5, 5), ebgp: false, ..path("10.9.0.0/16", &[65_005]) };
        let reflected = Path {
            from: Some(Ipv4Addr::new(10, 0, 0, 3)),
            from_id: Ipv4Addr::new(9, 9, 9, 9),
            originator_id: Some(Ipv4Addr::new(3, 3, 3, 3)),
            ..direct.clone()
        };
        assert!(better_std(&reflected, &direct) && !better_std(&direct, &reflected));
        let longer = Path {
            cluster_list: vec![Ipv4Addr::new(1, 0, 0, 1), Ipv4Addr::new(1, 0, 0, 2)],
            from: Some(Ipv4Addr::new(10, 0, 0, 4)),
            ..reflected.clone()
        };
        assert!(better_std(&reflected, &longer), "fewer clusters wins once the originators are equal");
    }

    #[test]
    fn a_confederation_keeps_the_as_path_inside_and_names_the_identifier_outside() {
        let a = speaker(65_001);
        let mut ab = conf::Bgp::new(65_001);
        ab.confed_id = Some(100);
        ab.confed_peers = vec![65_002];
        let (n, ext) = ext_peering();
        let across = Peering { confed: true, ..ext.clone() };
        let learned =
            Path { from: Some(Ipv4Addr::new(10, 0, 9, 9)), local_pref: 250, med: 7, confed_path: vec![65_003], ..path("10.9.0.0/16", &[65_005]) };
        // Across a member link: the AS path is left as it is, our member AS joins the confederation
        // path, the next hop, MED and local preference go along.
        let sent = export(&a, &ab, &n, &across, &learned).unwrap();
        assert_eq!(sent.as_path, vec![65_005]);
        assert_eq!(sent.confed_path, vec![65_001, 65_003]);
        assert_eq!((sent.local_pref, sent.med, sent.next_hop), (Some(250), Some(7), learned.next_hop));
        // Out of the confederation: the identifier in front, the confederation path gone.
        let outside = export(&a, &ab, &n, &ext, &learned).unwrap();
        assert_eq!(outside.as_path, vec![100, 65_005]);
        assert!(outside.confed_path.is_empty());
        // Not back through a member AS it already crossed.
        let seen = Path { confed_path: vec![65_002], ..learned.clone() };
        assert!(export(&a, &ab, &n, &across, &seen).is_none());
        // no-export crosses a member link (it stays in the confederation), local-AS does not.
        let no_export = Path { communities: vec![policy::NO_EXPORT], ..learned.clone() };
        assert!(export(&a, &ab, &n, &across, &no_export).is_some() && export(&a, &ab, &n, &ext, &no_export).is_none());
        let local_as = Path { communities: vec![policy::LOCAL_AS], ..learned };
        assert!(export(&a, &ab, &n, &across, &local_as).is_none());
        // What a member receives over a member link is kept as internal: local preference and all.
        let mut b = speaker(65_002);
        if let Some(bb) = b.bgp.as_mut() {
            bb.confed_id = Some(100);
            bb.confed_peers = vec![65_001];
        }
        let m = conf::Neighbor::new(Ipv4Addr::new(10, 0, 0, 1));
        let there = Peering { peer_addr: m.addr, local: Ipv4Addr::new(10, 0, 0, 2), ..across };
        let net: Ipv4Net = "10.9.0.0/16".parse().unwrap();
        let rid = Ipv4Addr::new(1, 1, 1, 1);
        let got = import(&b, &m, &there, &sent, rid, net, &[]).unwrap();
        assert_eq!((got.confed, got.ebgp, got.local_pref, got.confed_path.clone()), (true, false, 250, vec![65_001, 65_003]));
        assert!(!got.internal(), "a path from a member link may be passed to internal neighbours");
        // The member AS in the confederation path, or the identifier in the AS path, is a loop.
        let again = msg::PathAttrs { confed_path: vec![65_003, 65_002], ..sent.clone() };
        assert!(import(&b, &m, &there, &again, rid, net, &[]).is_none());
        let outer = msg::PathAttrs { as_path: vec![100, 65_005], ..sent };
        assert!(import(&b, &m, &there, &outer, rid, net, &[]).is_none());
    }

    #[test]
    fn private_as_numbers_are_removed_and_a_default_route_can_be_originated() {
        let a = speaker(65_001);
        let ab = conf::Bgp::new(65_001);
        let (mut n, p) = ext_peering();
        let learned = Path { from: Some(Ipv4Addr::new(10, 0, 9, 9)), ..path("10.9.0.0/16", &[64_512, 64_513]) };
        assert_eq!(export(&a, &ab, &n, &p, &learned).unwrap().as_path, vec![65_001, 64_512, 64_513]);
        n.remove_private_as = true;
        assert_eq!(export(&a, &ab, &n, &p, &learned).unwrap().as_path, vec![65_001], "an all-private path loses them all");
        let mixed = Path { as_path: vec![64_512, 200, 64_513], ..learned.clone() };
        assert_eq!(export(&a, &ab, &n, &p, &mixed).unwrap().as_path, vec![65_001, 64_512, 200, 64_513], "a mixed path is left alone");
        n.remove_private_all = true;
        assert_eq!(export(&a, &ab, &n, &p, &mixed).unwrap().as_path, vec![65_001, 200]);
        // Inside the AS nothing is removed.
        let n_int = conf::Neighbor { remote_as: Some(65_001), remove_private_as: true, ..conf::Neighbor::new(Ipv4Addr::new(10, 0, 5, 2)) };
        let int = Peering { ebgp: false, ttl: 255, ..p.clone() };
        assert_eq!(export(&a, &ab, &n_int, &int, &learned).unwrap().as_path, vec![64_512, 64_513]);
        // default-originate sends 0.0.0.0/0 with this router as the next hop, once.
        assert!(export_all(&a, &ab, &n, &p, &[], &[]).is_empty());
        n.default_originate = true;
        let sent = export_all(&a, &ab, &n, &p, &[], &[]);
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0].0.prefix, sent[0].1.as_path.clone(), sent[0].1.next_hop), (0, vec![65_001], p.local));
        let own_default = Path { net: "0.0.0.0/0".parse().unwrap(), ..learned.clone() };
        assert_eq!(export_all(&a, &ab, &n, &p, &[], &[&own_default]).len(), 1, "a default route already sent is not sent twice");
        let n_int = conf::Neighbor { default_originate: true, ..n_int };
        let inside = export_all(&a, &ab, &n_int, &int, &[], &[]);
        assert_eq!((inside[0].1.as_path.len(), inside[0].1.local_pref), (0, Some(100)));
    }

    #[test]
    fn always_compare_med_as_path_ignore_and_internal_multipath_change_the_choice() {
        let a = Path { med: 50, ..path("10.9.0.0/16", &[65_002]) };
        let b = Path { as_path: vec![65_003], med: 5, from: Some(Ipv4Addr::new(10, 0, 0, 6)), from_id: Ipv4Addr::new(9, 9, 9, 9), ..a.clone() };
        // The MED of paths from different neighbouring ASes is not compared unless told to.
        assert!(better_std(&a, &b));
        let always = Rank { always_med: true, ..Rank::default() };
        assert!(better(&b, &a, &always) && !better(&a, &b, &always));
        // The AS-path length counts unless told to leave it out.
        let longer = Path { as_path: vec![65_002, 65_009], from_id: Ipv4Addr::new(1, 1, 1, 1), from: Some(Ipv4Addr::new(10, 0, 0, 7)), ..a.clone() };
        assert!(better_std(&a, &longer));
        let ignore = Rank { ignore_len: true, ..Rank::default() };
        assert!(better(&longer, &a, &ignore));
        // Internal paths share the routing table only under `maximum-paths ibgp`.
        let via = |last: u8| Path {
            ebgp: false,
            from: Some(Ipv4Addr::new(10, 0, last, 2)),
            from_id: Ipv4Addr::new(last, last, last, last),
            next_hop: Ipv4Addr::new(10, 0, last, 2),
            resolved: Some((format!("g0/{last}"), Ipv4Addr::new(10, 0, last, 2))),
            ..path("10.9.0.0/16", &[65_002])
        };
        let paths = vec![via(1), via(2), via(3)];
        let count = |o: Rank| rank(paths.clone(), &o).iter().filter(|p| p.multi).count();
        assert_eq!(count(Rank { multi: 8, ..Rank::default() }), 0, "maximum-paths is for external paths");
        assert_eq!(count(Rank { multi_ibgp: 2, ..Rank::default() }), 1);
        assert_eq!(count(Rank { multi_ibgp: 8, ..Rank::default() }), 2);
    }

    #[test]
    fn a_next_hop_resolves_through_the_most_specific_route() {
        let ifc = L3If {
            port: 0,
            sub: None,
            vlan: None,
            net: "10.0.0.1/24".parse().unwrap(),
            mac: netlabcraft_core::MacAddr::local(DeviceId(1), 1),
            name: "g0/0".into(),
        };
        let row = |code, net: &str, via: Option<&str>, iface: Option<&str>, distance| RouteEntry {
            code,
            net: net.parse().unwrap(),
            via: via.map(|v| v.parse().unwrap()),
            iface: iface.map(String::from),
            distance,
            metric: 0,
            tag: 0,
        };
        let table = vec![
            row('C', "10.0.0.0/24", None, Some("g0/0"), 0),
            row('L', "10.0.0.1/32", None, Some("g0/0"), 0),
            row('S', "172.16.0.0/16", Some("10.0.0.9"), None, 1),
            row('O', "172.16.5.0/24", Some("10.0.0.7"), Some("g0/0"), 110),
        ];
        let ifs = vec![ifc];
        assert_eq!(resolve(&table, &ifs, "10.0.0.2".parse().unwrap()), Some(("g0/0".to_string(), "10.0.0.2".parse().unwrap())));
        assert_eq!(resolve(&table, &ifs, "172.16.1.1".parse().unwrap()), Some(("g0/0".to_string(), "10.0.0.9".parse().unwrap())));
        assert_eq!(resolve(&table, &ifs, "172.16.5.1".parse().unwrap()), Some(("g0/0".to_string(), "10.0.0.7".parse().unwrap())));
        // Our own address and an address nothing routes are not next hops.
        assert_eq!(resolve_metric(&table, &ifs, "172.16.5.1".parse().unwrap()).map(|r| r.2), Some(0));
        assert_eq!(resolve(&table, &ifs, "10.0.0.1".parse().unwrap()), None);
        assert_eq!(resolve(&table, &ifs, "8.8.8.8".parse().unwrap()), None);
    }
}
