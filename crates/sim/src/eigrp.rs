//! EIGRP (RFC 7868) for IPv4, computed the way DUAL converges.
//!
//! Interfaces whose address falls under a `network` statement run EIGRP. Two routers are
//! neighbours when such interfaces (not passive) share a subnet, reach each other at layer 2 and
//! belong to the same autonomous system. Each router starts from its EIGRP networks and, round
//! after round, takes every route its neighbours use, adding the link it hears it on: the path's
//! bandwidth is the slowest link's, its delay the sum of the links' delays, and the composite
//! metric (K1 = K3 = 1) is `256 × (10⁷ / bandwidth in kbit/s + delay in tens of µs)` (RFC 7868
//! §5.6.2). The lowest is the feasible distance and every neighbour offering it is a successor;
//! any other neighbour whose reported distance is below the feasible distance is a feasible
//! successor (the feasibility condition, §5.3). Up to `maximum-paths` successors are installed
//! (equal-cost load balancing); with `variance N`, feasible successors whose distance is below
//! N times the feasible distance are installed too. Split horizon: a router doesn't offer a
//! route back to the neighbour it uses for it.
//!
//! Timers run in simulated time (see [`Sim::eigrp_refresh`]): hellos go out at each interface's
//! hello interval and announce its hold time. A neighbour that stops being heard while the
//! interface towards it stays up (a switch port shut in between, a cable pulled at the far
//! end) is kept for the hold time it announced, counted from its last hello, and only then
//! dropped; a neighbour whose own interface is shut down, or whose EIGRP stops, says goodbye
//! and is dropped at once. Each change sends updates, queries (for destinations left without a
//! feasible successor) and replies on the wire, each acknowledged.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::eigrp::{
    EIGRP_ROUTERS, EigrpPacket, FLAG_EOT, FLAG_INIT, InternalRoute, OPCODE_QUERY, OPCODE_REPLY, OPCODE_UPDATE, PROTO_EIGRP, Tlv,
};
use netlabcraft_wire::{Ipv4, control};

use crate::{L3If, Sim, l3_ifaces, up_ifaces};

/// Administrative distance of internal EIGRP routes.
pub const DISTANCE: u8 = 90;
/// Hold time announced in hellos by default (seconds).
pub const HOLD_SECS: u16 = netlabcraft_wire::eigrp::HOLD_SECS;
/// Hello interval by default (seconds).
pub const HELLO_SECS: u16 = netlabcraft_wire::eigrp::HELLO_SECS;
/// The metric that means unreachable.
pub const INFINITY: u32 = u32::MAX;
/// Most neighbours held past their last hello at once.
const MAX_HELD: usize = 1_024;
/// Most routes in one update, query or reply.
const ROUTES_PER_PACKET: usize = 32;

/// An interface's bandwidth (kbit/s) and delay (tens of µs): configured, else by its type.
pub fn if_metrics(d: &Device, ifc: &L3If) -> (u32, u32) {
    let port = d.ports.get(ifc.port);
    let (bw, delay) = default_metrics(&ifc.name);
    let bw = port.and_then(|p| p.bandwidth).unwrap_or(bw).max(1);
    let delay = port.and_then(|p| p.delay).unwrap_or(delay);
    (bw, delay)
}

/// The usual bandwidth and delay of an interface type.
pub fn default_metrics(name: &str) -> (u32, u32) {
    if name.starts_with("TenGigabitEthernet") {
        (10_000_000, 1)
    } else if name.starts_with("GigabitEthernet") || name.starts_with("Vlan") {
        (1_000_000, 1)
    } else if name.starts_with("FastEthernet") {
        (100_000, 10)
    } else if name.starts_with("Serial") {
        (1_544, 2_000)
    } else if name.starts_with("Loopback") {
        (8_000_000, 500)
    } else if name.starts_with("Tunnel") {
        (100, 5_000)
    } else {
        (10_000, 100)
    }
}

/// The composite metric of a path's slowest bandwidth and total delay.
pub fn metric(bw_kbit: u32, delay_tens_us: u64) -> u32 {
    let scaled = 10_000_000u64 / u64::from(bw_kbit.max(1));
    let m = scaled.saturating_add(delay_tens_us).saturating_mul(256);
    u32::try_from(m).unwrap_or(INFINITY - 1).min(INFINITY - 1)
}

/// An interface's hello interval and hold time (seconds) for the router's EIGRP process.
pub fn timers(d: &Device, iface: &str) -> (u16, u16) {
    let asn = d.eigrp.as_ref().map_or(0, |e| e.asn);
    let conf = d.eigrp_if.get(iface);
    let pick = |v: Option<(u16, u16)>, def: u16| v.filter(|(a, _)| *a == asn).map_or(def, |(_, s)| s.max(1));
    (pick(conf.and_then(|c| c.hello), HELLO_SECS), pick(conf.and_then(|c| c.hold), HOLD_SECS))
}

/// An EIGRP interface of a router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EigrpIf {
    pub name: String,
    pub net: Ipv4Net,
    pub bandwidth: u32,
    pub delay: u32,
    pub passive: bool,
    pub hello_secs: u16,
    pub hold_secs: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub device: DeviceId,
    pub address: Ipv4Addr,
    /// This router's interface towards it.
    pub iface: String,
    /// The hold time and hello interval the neighbour announces on its interface.
    pub hold_secs: u16,
    pub hello_secs: u16,
    /// No longer heard: kept until this simulated time (µs), then dropped.
    pub held_until: Option<u64>,
}

/// A route EIGRP installs: one per path in use (several for load balancing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EigrpRoute {
    pub net: Ipv4Net,
    pub via: Ipv4Addr,
    pub iface: String,
    /// This path's distance (the feasible distance for a successor).
    pub metric: u32,
}

/// One way to a destination in the topology table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    /// The neighbour's address; `None` for a network on one of this router's interfaces.
    pub via: Option<Ipv4Addr>,
    pub iface: String,
    /// The distance through this neighbour (computed distance).
    pub distance: u32,
    /// The distance the neighbour reports (its own feasible distance).
    pub reported: u32,
    pub successor: bool,
    /// In the routing table (a successor within `maximum-paths`, or a feasible successor
    /// `variance` admits).
    pub installed: bool,
}

/// A destination in the topology table: its feasible distance, successors and feasible
/// successors (in that order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopoEntry {
    pub net: Ipv4Net,
    pub fd: u32,
    pub paths: Vec<Path>,
}

/// EIGRP's view of the lab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EigrpState {
    pub router_ids: BTreeMap<DeviceId, Ipv4Addr>,
    pub neighbors: BTreeMap<DeviceId, Vec<Neighbor>>,
    pub interfaces: BTreeMap<DeviceId, Vec<EigrpIf>>,
    pub topology: BTreeMap<DeviceId, Vec<TopoEntry>>,
    pub routes: BTreeMap<DeviceId, Vec<EigrpRoute>>,
    /// Summaries a router advertises (`ip summary-address eigrp`), with their metrics; each is
    /// also a route to Null0 among its routes.
    pub summaries: BTreeMap<DeviceId, Vec<(Ipv4Net, u32)>>,
}

/// The interface name summary routes point at: they discard what nothing more specific takes.
pub const NULL0: &str = "Null0";

/// A router's IPv4 summaries for its autonomous system on an interface.
fn summaries_on(d: &Device, iface: &str) -> Vec<Ipv4Net> {
    let asn = d.eigrp.as_ref().map_or(0, |e| e.asn);
    d.eigrp_summary.get(iface).map(|s| s.v4_for(asn)).unwrap_or_default()
}

/// A neighbour no longer heard, kept until its hold time runs out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub router: DeviceId,
    pub iface: String,
    pub address: Ipv4Addr,
    pub peer: DeviceId,
    pub until: u64,
}

/// The router id: configured, else the highest interface address.
pub fn router_id(d: &Device) -> Option<Ipv4Addr> {
    let e = d.eigrp.as_ref()?;
    e.router_id.or_else(|| crate::loopback::default_router_id(d))
}

fn is_passive(d: &Device, name: &str) -> bool {
    d.eigrp.as_ref().is_some_and(|e| e.passive.iter().any(|p| netlabcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == name)))
}

fn eigrp_ifaces(t: &Topology, d: &Device) -> Vec<(L3If, bool)> {
    let Some(e) = &d.eigrp else { return Vec::new() };
    up_ifaces(t, d).into_iter().filter(|i| e.covers(i.net.addr)).map(|i| (i.clone(), is_passive(d, &i.name))).collect()
}

/// A router's best way to a network while computing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Best {
    bw: u32,
    delay: u64,
    metric: u32,
    hops: u8,
    /// (our interface, neighbour's address, neighbour) unless connected.
    via: Option<(String, Ipv4Addr, DeviceId)>,
}

/// Who hears whom: (our interface, its bandwidth and delay, their address, their device).
type Adj = (L3If, u32, u32, Ipv4Addr, DeviceId);

/// Rounds of updates at most (each round carries routes one router further).
const MAX_ROUNDS: usize = 128;

/// EIGRP converged over the lab as it is now, every neighbour heard.
pub fn compute(t: &Topology) -> EigrpState {
    compute_with(t, &[])
}

/// [`compute`] with neighbours that are no longer heard but still within their hold time.
pub fn compute_with(t: &Topology, held: &[Held]) -> EigrpState {
    let mut st = EigrpState::default();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| d.routes_ip() && d.eigrp.is_some()).collect();
    let mut ifs: BTreeMap<DeviceId, Vec<(L3If, bool)>> = BTreeMap::new();
    for d in &routers {
        let list = eigrp_ifaces(t, d);
        st.interfaces.insert(
            d.id,
            list.iter()
                .map(|(i, p)| {
                    let (bandwidth, delay) = if_metrics(d, i);
                    let (hello_secs, hold_secs) = timers(d, &i.name);
                    EigrpIf { name: i.name.clone(), net: i.net.network(), bandwidth, delay, passive: *p, hello_secs, hold_secs }
                })
                .collect(),
        );
        if let Some(r) = router_id(d) {
            st.router_ids.insert(d.id, r);
        }
        ifs.insert(d.id, list);
    }
    let asn = |d: &Device| d.eigrp.as_ref().map_or(0, |e| e.asn);
    let mut adj: BTreeMap<DeviceId, Vec<Adj>> = BTreeMap::new();
    for a in &routers {
        for (ia, pa) in ifs.get(&a.id).into_iter().flatten() {
            if *pa {
                continue;
            }
            let reach = crate::ospf::iface_reach(t, a, ia);
            let (bw, delay) = if_metrics(a, ia);
            for b in routers.iter().filter(|b| b.id != a.id && asn(b) == asn(a)) {
                for (ib, pb) in ifs.get(&b.id).into_iter().flatten() {
                    if !*pb
                        && ib.net.network() == ia.net.network()
                        && (ib.vlan == ia.vlan || ia.port == crate::SVI_PORT || ib.port == crate::SVI_PORT)
                        && crate::ospf::reaches(t, &reach, b, ib)
                    {
                        let (hello_secs, hold_secs) = timers(b, &ib.name);
                        adj.entry(a.id).or_default().push((ia.clone(), bw, delay, ib.net.addr, b.id));
                        st.neighbors.entry(a.id).or_default().push(Neighbor {
                            device: b.id,
                            address: ib.net.addr,
                            iface: ia.name.clone(),
                            hold_secs,
                            hello_secs,
                            held_until: None,
                        });
                    }
                }
            }
        }
    }
    // Neighbours still within their hold time: as if heard.
    for h in held {
        let Some((ia, _)) = ifs.get(&h.router).into_iter().flatten().find(|(i, p)| i.name == h.iface && !*p) else { continue };
        if adj.get(&h.router).is_some_and(|l| l.iter().any(|x| x.0.name == h.iface && x.3 == h.address)) {
            continue;
        }
        let Some(a) = t.device(h.router) else { continue };
        let (bw, delay) = if_metrics(a, ia);
        let (hello_secs, hold_secs) = t
            .device(h.peer)
            .and_then(|b| l3_ifaces(b).into_iter().find(|i| i.net.addr == h.address).map(|i| timers(b, &i.name)))
            .unwrap_or((HELLO_SECS, HOLD_SECS));
        adj.entry(h.router).or_default().push((ia.clone(), bw, delay, h.address, h.peer));
        st.neighbors.entry(h.router).or_default().push(Neighbor {
            device: h.peer,
            address: h.address,
            iface: h.iface.clone(),
            hold_secs,
            hello_secs,
            held_until: Some(h.until),
        });
    }
    // Each router's own networks.
    let mut connected: BTreeMap<DeviceId, BTreeMap<Ipv4Net, Best>> = BTreeMap::new();
    for d in &routers {
        let mut tbl: BTreeMap<Ipv4Net, Best> = BTreeMap::new();
        for (i, _) in ifs.get(&d.id).into_iter().flatten() {
            let (bw, delay) = if_metrics(d, i);
            let b = Best { bw, delay: u64::from(delay), metric: metric(bw, u64::from(delay)), hops: 0, via: None };
            if tbl.get(&i.net.network()).is_none_or(|old| b.metric < old.metric) {
                tbl.insert(i.net.network(), b);
            }
        }
        connected.insert(d.id, tbl);
    }
    // The summaries each neighbour applies out of its interface (by its address).
    let sums_of = |dev: DeviceId, addr: Ipv4Addr| -> Vec<Ipv4Net> {
        let Some(b) = t.device(dev) else { return Vec::new() };
        l3_ifaces(b).into_iter().find(|i| i.net.addr == addr).map(|i| summaries_on(b, &i.name)).unwrap_or_default()
    };
    let offer = |from: &BTreeMap<Ipv4Net, Best>, me: DeviceId, (_, bw, delay, addr, dev): &Adj| -> Vec<(Ipv4Net, Best, u32)> {
        // Split horizon: nothing offered back to the neighbour it goes through.
        let mut view: BTreeMap<Ipv4Net, Best> =
            from.iter().filter(|(_, b)| !b.via.as_ref().is_some_and(|(_, _, d)| *d == me)).map(|(n, b)| (*n, b.clone())).collect();
        // A summary that covers something replaces it, with the best metric under it.
        for s in sums_of(*dev, *addr) {
            let covered: Vec<Ipv4Net> = view.keys().filter(|n| n.prefix >= s.prefix && s.contains(n.addr)).copied().collect();
            let Some(best) = covered.iter().filter_map(|n| view.get(n)).min_by_key(|b| b.metric).cloned() else { continue };
            for n in &covered {
                view.remove(n);
            }
            view.insert(s, best);
        }
        view.iter()
            .map(|(net, b)| {
                let nbw = b.bw.min(*bw);
                let nd = b.delay.saturating_add(u64::from(*delay));
                (*net, Best { bw: nbw, delay: nd, metric: metric(nbw, nd), hops: b.hops.saturating_add(1), via: None }, b.metric)
            })
            .collect()
    };
    let mut tables = connected.clone();
    for _ in 0..MAX_ROUNDS {
        let snapshot = tables.clone();
        let mut next: BTreeMap<DeviceId, BTreeMap<Ipv4Net, Best>> = BTreeMap::new();
        for d in &routers {
            let mut tbl = connected.get(&d.id).cloned().unwrap_or_default();
            for a in adj.get(&d.id).into_iter().flatten() {
                let Some(from) = snapshot.get(&a.4) else { continue };
                for (net, mut cand, _) in offer(from, d.id, a) {
                    cand.via = Some((a.0.name.clone(), a.3, a.4));
                    let better = match tbl.get(&net) {
                        None => true,
                        Some(Best { via: None, .. }) => false,
                        Some(old) => cand.metric < old.metric || (cand.metric == old.metric && old.via.as_ref().is_some_and(|(_, oa, _)| a.3 < *oa)),
                    };
                    if better && cand.metric < INFINITY - 1 {
                        tbl.insert(net, cand);
                    }
                }
            }
            next.insert(d.id, tbl);
        }
        let done = next == tables;
        tables = next;
        if done {
            break;
        }
    }
    // The topology table: successors and feasible successors; the routes installed.
    for d in &routers {
        let Some(tbl) = tables.get(&d.id) else { continue };
        let (max_paths, variance) = d.eigrp.as_ref().map_or((4, 1), |e| (e.max_paths(), u64::from(e.variance.unwrap_or(1).max(1))));
        let mut entries = Vec::new();
        let mut routes = Vec::new();
        for (net, best) in tbl {
            let fd = best.metric;
            let mut paths = Vec::new();
            match &best.via {
                None => {
                    let name =
                        ifs.get(&d.id).into_iter().flatten().find(|(i, _)| i.net.network() == *net).map_or(String::new(), |(i, _)| i.name.clone());
                    paths.push(Path { via: None, iface: name, distance: fd, reported: 0, successor: true, installed: true });
                }
                Some((_, addr, _)) => {
                    for a in adj.get(&d.id).into_iter().flatten() {
                        let Some(from) = tables.get(&a.4) else { continue };
                        for (n, cand, reported) in offer(from, d.id, a) {
                            if n != *net {
                                continue;
                            }
                            let successor = cand.metric == fd;
                            if successor || reported < fd {
                                paths.push(Path {
                                    via: Some(a.3),
                                    iface: a.0.name.clone(),
                                    distance: cand.metric,
                                    reported,
                                    successor,
                                    installed: false,
                                });
                            }
                        }
                    }
                    // The successor DUAL picked first, then the others by distance.
                    paths.sort_by_key(|p| (p.via != Some(*addr), !p.successor, p.distance, p.via));
                    paths.dedup_by(|a, b| a.via == b.via && a.iface == b.iface);
                    let limit = u64::from(fd).saturating_mul(variance);
                    let mut used = 0usize;
                    for p in &mut paths {
                        let admitted = p.successor || (variance > 1 && u64::from(p.distance) < limit);
                        if admitted && used < max_paths {
                            p.installed = true;
                            used += 1;
                            if let Some(v) = p.via {
                                routes.push(EigrpRoute { net: *net, via: v, iface: p.iface.clone(), metric: p.distance });
                            }
                        }
                    }
                }
            }
            entries.push(TopoEntry { net: *net, fd, paths });
        }
        // Each summary with something under it: a route to Null0 here.
        let mut sums: Vec<(Ipv4Net, u32)> = Vec::new();
        for (i, _) in ifs.get(&d.id).into_iter().flatten() {
            for s in summaries_on(d, &i.name) {
                let under = tbl.iter().filter(|(n, _)| n.prefix >= s.prefix && s.contains(n.addr) && **n != s).map(|(_, b)| b.metric).min();
                if let Some(m) = under
                    && !sums.iter().any(|(x, _)| *x == s)
                {
                    sums.push((s, m));
                    routes.push(EigrpRoute { net: s, via: Ipv4Addr::UNSPECIFIED, iface: NULL0.to_string(), metric: m });
                }
            }
        }
        st.summaries.insert(d.id, sums);
        st.topology.insert(d.id, entries);
        st.routes.insert(d.id, routes);
    }
    st
}

/// Whether a neighbour that is no longer reachable said goodbye (its interface was shut down or
/// its EIGRP stopped there) rather than just falling silent.
fn said_goodbye(t: &Topology, me: &Device, n: &Neighbor) -> bool {
    let Some(b) = t.device(n.device) else { return false };
    let same_as = match (&b.eigrp, &me.eigrp) {
        (Some(x), Some(y)) => x.asn == y.asn,
        _ => false,
    };
    let Some(ib) = l3_ifaces(b).into_iter().find(|i| i.net.addr == n.address) else { return true };
    !same_as || !b.eigrp.as_ref().is_some_and(|e| e.covers(ib.net.addr)) || is_passive(b, &ib.name)
}

/// Feasible distance and installed next hops per destination of one router.
fn summary(st: &EigrpState, d: DeviceId) -> BTreeMap<Ipv4Net, (u32, BTreeSet<Ipv4Addr>, BTreeSet<Ipv4Addr>)> {
    st.topology
        .get(&d)
        .into_iter()
        .flatten()
        .map(|e| {
            let succ = e.paths.iter().filter(|p| p.installed).filter_map(|p| p.via).collect();
            let feasible = e.paths.iter().filter_map(|p| p.via).collect();
            (e.net, (e.fd, succ, feasible))
        })
        .collect()
}

/// The route a router announces for a destination on an interface (split horizon applied:
/// unreachable when its successor is out of that interface).
fn announce(st: &EigrpState, d: &Device, net: Ipv4Net, out: &str) -> InternalRoute {
    let Some(e) = st.topology.get(&d.id).and_then(|l| l.iter().find(|e| e.net == net)) else { return InternalRoute::unreachable(net) };
    let Some(p) = e.paths.first() else { return InternalRoute::unreachable(net) };
    if p.via.is_some() && p.iface == out {
        return InternalRoute::unreachable(net);
    }
    // The metric fields: the slowest bandwidth and the delay, back from the composite metric
    // with the interface's own bandwidth (close enough for what the packet shows).
    let ifc = st.interfaces.get(&d.id).and_then(|l| l.iter().find(|i| i.name == p.iface));
    let bw = ifc.map_or(1_000_000, |i| i.bandwidth);
    let scaled = 10_000_000u64 / u64::from(bw.max(1));
    let delay = (u64::from(e.fd) / 256).saturating_sub(scaled);
    InternalRoute::new(net, bw, delay, u8::from(p.via.is_some()))
}

impl Sim {
    /// Brings EIGRP's state up to date at the current simulated time: recomputes after the lab
    /// changed, keeps neighbours that fell silent until their hold time runs out, drops them
    /// when it does, logs neighbour changes and puts the resulting updates, queries and replies
    /// on the wire.
    pub(crate) fn eigrp_refresh(&mut self, t: &Topology) {
        if !t.devices.iter().any(|d| d.eigrp.is_some()) {
            self.eigrp = Some(EigrpState::default());
            self.eigrp_held.clear();
            return;
        }
        let now = self.now;
        let f = crate::ospf::fingerprint(t);
        let expired: Vec<Held> = self.eigrp_held.iter().filter(|h| h.until <= now).cloned().collect();
        if self.eigrp.is_some() && self.eigrp_for == f && expired.is_empty() {
            return;
        }
        self.eigrp_held.retain(|h| h.until > now);
        self.eigrp_for = f;
        let old = self.eigrp.take();
        let raw = compute(t);
        let heard =
            |st: &EigrpState, h: &Held| st.neighbors.get(&h.router).is_some_and(|l| l.iter().any(|n| n.iface == h.iface && n.address == h.address));
        // Neighbours heard again, or whose interface here is down, are no longer held.
        self.eigrp_held
            .retain(|h| !heard(&raw, h) && raw.interfaces.get(&h.router).is_some_and(|l| l.iter().any(|i| i.name == h.iface && !i.passive)));
        let mut down: Vec<(DeviceId, Ipv4Addr, String, &'static str)> =
            expired.iter().map(|h| (h.router, h.address, h.iface.clone(), "hold time expired")).collect();
        if let Some(old) = &old {
            for (me, list) in &old.neighbors {
                let Some(d) = t.device(*me) else { continue };
                for n in list {
                    let h = Held { router: *me, iface: n.iface.clone(), address: n.address, peer: n.device, until: 0 };
                    if heard(&raw, &h) || self.eigrp_held.iter().any(|x| x.router == *me && x.iface == n.iface && x.address == n.address) {
                        continue;
                    }
                    if expired.iter().any(|x| x.router == *me && x.iface == n.iface && x.address == n.address) {
                        continue;
                    }
                    let mine_up = raw.interfaces.get(me).is_some_and(|l| l.iter().any(|i| i.name == n.iface && !i.passive));
                    if !mine_up || d.eigrp.is_none() {
                        down.push((*me, n.address, n.iface.clone(), "interface down"));
                    } else if said_goodbye(t, d, n) {
                        down.push((*me, n.address, n.iface.clone(), "goodbye received"));
                    } else {
                        // Silent: kept from its last hello for the hold time it announced.
                        let hello = u64::from(n.hello_secs.max(1)) * 1_000_000;
                        let last = now - now % hello;
                        let until = last.saturating_add(u64::from(n.hold_secs) * 1_000_000);
                        if until > now && self.eigrp_held.len() < MAX_HELD {
                            self.eigrp_held.push(Held { until, ..h });
                        } else {
                            down.push((*me, n.address, n.iface.clone(), "hold time expired"));
                        }
                    }
                }
            }
        }
        let new = if self.eigrp_held.is_empty() { raw } else { compute_with(t, &self.eigrp_held) };
        let asn_of = |id: DeviceId| t.device(id).and_then(|d| d.eigrp.as_ref()).map_or(0, |e| e.asn);
        for (me, addr, iface, why) in down {
            if t.device(me).is_some_and(|d| d.eigrp.is_some()) {
                self.log(me, &format!("EIGRP AS {}: neighbour {addr} ({iface}) is down: {why}", asn_of(me)));
            }
        }
        if let Some(old) = &old {
            for (me, list) in &new.neighbors {
                for n in
                    list.iter().filter(|n| !old.neighbors.get(me).is_some_and(|o| o.iter().any(|x| x.iface == n.iface && x.address == n.address)))
                {
                    self.log(*me, &format!("EIGRP AS {}: neighbour {} ({}) is up: new adjacency", asn_of(*me), n.address, n.iface));
                }
            }
            self.eigrp_packets(t, old, &new, now);
        }
        self.eigrp = Some(new);
    }

    /// When the next held neighbour's hold time runs out.
    pub(crate) fn eigrp_next_expiry(&self) -> Option<u64> {
        self.eigrp_held.iter().map(|h| h.until).min()
    }

    pub(crate) fn eigrp_seq(&mut self, d: DeviceId) -> u32 {
        let s = self.eigrp_seqs.entry(d).or_insert(0);
        *s = s.wrapping_add(1).max(1);
        *s
    }

    /// The packets DUAL sends going from `old` to `new`: an initial update to each new
    /// neighbour; a query for each destination left without a feasible successor, answered by
    /// every neighbour; an update for each destination whose distance changed. Each is
    /// acknowledged.
    fn eigrp_packets(&mut self, t: &Topology, old: &EigrpState, new: &EigrpState, at: u64) {
        // (time offset, router, interface, unicast neighbour or multicast, opcode, flags, routes)
        type Out = (u64, DeviceId, String, Option<Ipv4Addr>, u8, u32, Vec<InternalRoute>);
        let mut out: Vec<Out> = Vec::new();
        for (me, ifs) in &new.interfaces {
            let Some(d) = t.device(*me) else { continue };
            let before = summary(old, *me);
            let after = summary(new, *me);
            let nbrs = new.neighbors.get(me).cloned().unwrap_or_default();
            // New neighbours get the whole table.
            for n in nbrs.iter().filter(|n| n.held_until.is_none()) {
                if old.neighbors.get(me).is_some_and(|o| o.iter().any(|x| x.iface == n.iface && x.address == n.address)) {
                    continue;
                }
                let routes: Vec<InternalRoute> = after.keys().map(|net| announce(new, d, *net, &n.iface)).filter(|r| !r.is_unreachable()).collect();
                out.push((1_000, *me, n.iface.clone(), Some(n.address), OPCODE_UPDATE, FLAG_INIT | FLAG_EOT, routes));
            }
            let mut queried = Vec::new();
            let mut changed = Vec::new();
            for (net, (fd, succ, _)) in &before {
                match after.get(net) {
                    None => queried.push(*net),
                    Some((nfd, nsucc, _)) if nfd != fd || nsucc != succ => {
                        // Going active when the new successor wasn't a feasible successor before.
                        let feasible_before = before.get(net).map(|b| &b.2);
                        if !succ.is_empty() && !nsucc.iter().all(|s| feasible_before.is_some_and(|f| f.contains(s))) && nfd > fd {
                            queried.push(*net);
                        } else {
                            changed.push(*net);
                        }
                    }
                    _ => {}
                }
            }
            changed.extend(after.keys().filter(|n| !before.contains_key(n)).copied());
            for ifc in ifs.iter().filter(|i| !i.passive) {
                if !nbrs.iter().any(|n| n.iface == ifc.name) {
                    continue;
                }
                if !queried.is_empty() {
                    let routes = queried.iter().map(|n| InternalRoute::unreachable(*n)).collect();
                    out.push((2_000, *me, ifc.name.clone(), None, OPCODE_QUERY, 0, routes));
                    // Every neighbour there answers with its own distance.
                    for n in nbrs.iter().filter(|n| n.iface == ifc.name && n.held_until.is_none()) {
                        let Some(b) = t.device(n.device) else { continue };
                        let Some(bif) = new.interfaces.get(&n.device).and_then(|l| {
                            l.iter().find(|i| l3_ifaces(b).iter().any(|x| x.name == i.name && x.net.addr == n.address)).map(|i| i.name.clone())
                        }) else {
                            continue;
                        };
                        let Some(my_addr) = new.neighbors.get(&n.device).and_then(|l| l.iter().find(|x| x.device == *me).map(|x| x.address)) else {
                            continue;
                        };
                        let routes = queried.iter().map(|net| announce(new, b, *net, &bif)).collect();
                        out.push((3_000, n.device, bif, Some(my_addr), OPCODE_REPLY, 0, routes));
                    }
                }
                if !changed.is_empty() {
                    let routes = changed.iter().map(|net| announce(new, d, *net, &ifc.name)).collect();
                    out.push((4_000, *me, ifc.name.clone(), None, OPCODE_UPDATE, 0, routes));
                }
            }
        }
        for (offset, dev, iface, to, opcode, flags, routes) in out {
            let Some(d) = t.device(dev) else { continue };
            let asn = d.eigrp.as_ref().map_or(0, |e| e.asn);
            let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == iface) else { continue };
            for (k, chunk) in (0u64..).zip(routes.chunks(ROUTES_PER_PACKET).take(64)) {
                let seq = self.eigrp_seq(dev);
                let p = EigrpPacket { opcode, flags, seq, ack: 0, asn, tlvs: chunk.iter().map(|r| Tlv::Internal(*r)).collect() };
                let when = at.saturating_add(offset).saturating_add(k * 100);
                self.eigrp_send(t, d, &ifc, to, p.encode(), when);
                // Each receiver acknowledges.
                let receivers: Vec<(DeviceId, Ipv4Addr)> = new
                    .neighbors
                    .get(&dev)
                    .into_iter()
                    .flatten()
                    .filter(|n| n.iface == iface && n.held_until.is_none() && to.is_none_or(|a| a == n.address))
                    .map(|n| (n.device, n.address))
                    .collect();
                for (rd, raddr) in receivers {
                    let Some(r) = t.device(rd) else { continue };
                    let Some(rif) = l3_ifaces(r).into_iter().find(|i| i.net.addr == raddr) else { continue };
                    let ack = EigrpPacket::ack(asn, seq).encode();
                    self.eigrp_send(t, r, &rif, Some(ifc.net.addr), ack, when.saturating_add(500));
                }
            }
        }
    }

    /// Queues an EIGRP packet out of an interface: to `224.0.0.10`, or unicast to a neighbour.
    fn eigrp_send(&mut self, t: &Topology, d: &Device, ifc: &L3If, to: Option<Ipv4Addr>, payload: Vec<u8>, at: u64) {
        let (dst, mac) = match to {
            None => (EIGRP_ROUTERS, control::ipv4_multicast_mac(EIGRP_ROUTERS)),
            Some(a) => {
                let mac = t
                    .devices
                    .iter()
                    .find_map(|x| l3_ifaces(x).into_iter().find(|i| i.net.addr == a && i.net.network() == ifc.net.network()).map(|i| i.mac));
                (a, mac.unwrap_or(control::ipv4_multicast_mac(EIGRP_ROUTERS)))
            }
        };
        let ip = Ipv4 { id: 0, ttl: 2, protocol: PROTO_EIGRP, src: ifc.net.addr, dst, payload };
        self.queue_l3(t, d, ifc, ifc.mac, mac, ip.encode(), at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_composite_metric_takes_the_slowest_link_and_the_total_delay() {
        // Gigabit Ethernet: 256 × (10 + 1).
        assert_eq!(metric(1_000_000, 1), 2_816);
        // A T1 serial link: 256 × (6476 + 2000).
        assert_eq!(metric(1_544, 2_000), 2_169_856);
        // Hostile values saturate below infinity instead of overflowing.
        assert_eq!(metric(0, u64::MAX), INFINITY - 1);
        assert_eq!(default_metrics("FastEthernet0/1"), (100_000, 10));
        assert_eq!(default_metrics("Serial0/0/0"), (1_544, 2_000));
    }
}
