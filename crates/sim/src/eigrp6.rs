//! EIGRP for IPv6 (RFC 7868), computed from the topology like the IPv4 process.
//!
//! A router runs it with `ipv6 router eigrp N` (and a 32-bit router id: `eigrp router-id`, else
//! its highest IPv4 address; without one the process doesn't start) and interfaces join with
//! `ipv6 eigrp N`; there are no `network` statements. Neighbours meet over link-local addresses
//! on a shared link in the same autonomous system (their global prefixes need not match), and
//! routes lead to the neighbour's link-local address. Each router advertises the global prefixes
//! of its EIGRP interfaces with that interface's bandwidth and delay; the metric is the classic
//! composite one (`256 × (10⁷ / slowest kbit/s + total delay in tens of µs)`, K1 = K3 = 1), the
//! lowest is the feasible distance, every neighbour offering it is a successor, and any other
//! whose reported distance is below the feasible distance is a feasible successor (§5.3). Up to
//! `maximum-paths` successors are installed; with `variance N`, feasible successors within N
//! times the feasible distance too. `ipv6 summary-address eigrp N PREFIX` on an interface
//! advertises the summary there instead of the prefixes under it (with the best of their
//! metrics), and puts a route to Null0 for it in the router's own table. Hellos go to
//! `FF02::A` every hello interval (5 s; `ipv6 hello-interval eigrp N S` per interface) announcing
//! the hold time (15 s; `ipv6 hold-time eigrp N S`) (`control`); a neighbour that falls silent
//! while both interfaces stay up keeps its routes until its hold time runs out
//! ([`Sim::eigrp6_refresh`]); each change puts DUAL's updates, queries and replies on the wire,
//! acknowledged ([`Sim::eigrp6_packets`]).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use netlabcraft_core::{DeviceId, Ipv6Net};
use netlabcraft_model::{Device, DeviceKind, End, Topology};
use netlabcraft_wire::eigrp::{
    EIGRP_ROUTERS6, EigrpPacket, FLAG_EOT, FLAG_INIT, InternalRoute6, OPCODE_QUERY, OPCODE_REPLY, OPCODE_UPDATE, PROTO_EIGRP, Tlv,
};

use crate::eigrp::{HELLO_SECS, HOLD_SECS};
use crate::eigrp::{default_metrics, metric};
use crate::v6::{L3If6, conf6, l3_ifaces6, routes6, up_ifaces6};
use crate::{SVI_PORT, Sim, port_up};

/// Most neighbours held at once.
const MAX_HELD: usize = 1_024;

/// A neighbour heard on one interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor6 {
    pub device: DeviceId,
    /// Their link-local address.
    pub address: Ipv6Addr,
    /// Our interface towards them.
    pub iface: String,
    /// The hold time and hello interval the neighbour announces on its interface.
    pub hold_secs: u16,
    pub hello_secs: u16,
    /// No longer heard: kept until this simulated time (µs), then dropped.
    pub held_until: Option<u64>,
}

/// A neighbour no longer heard, kept until its hold time runs out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub router: DeviceId,
    pub iface: String,
    pub address: Ipv6Addr,
    pub peer: DeviceId,
    /// The neighbour's interface towards us.
    pub peer_iface: String,
    pub until: u64,
}

/// An interface's hello interval and hold time (seconds) for the router's EIGRP for IPv6 process.
pub fn timers(d: &Device, iface: &str) -> (u16, u16) {
    let asn = d.eigrp6.as_ref().map_or(0, |e| e.asn);
    let conf = d.eigrp_if.get(iface);
    let pick = |v: Option<(u16, u16)>, def: u16| v.filter(|(a, _)| *a == asn).map_or(def, |(_, s)| s.max(1));
    (pick(conf.and_then(|c| c.hello6), HELLO_SECS), pick(conf.and_then(|c| c.hold6), HOLD_SECS))
}

fn is_passive(d: &Device, name: &str) -> bool {
    d.eigrp6.as_ref().is_some_and(|e| e.passive.iter().any(|p| netlabcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == name)))
}

/// Whether a neighbour that is no longer reachable said goodbye (its interface was shut down or
/// its EIGRP stopped there) rather than just falling silent.
fn said_goodbye(t: &Topology, me: &Device, n: &Neighbor6) -> bool {
    let Some(b) = t.device(n.device) else { return true };
    let same_as = matches!((&b.eigrp6, &me.eigrp6), (Some(x), Some(y)) if x.asn == y.asn);
    let Some(ib) = l3_ifaces6(b).into_iter().find(|i| i.link_local == n.address) else { return true };
    !same_as || !running(b) || conf6(b, &ib).and_then(|c| c.eigrp) != b.eigrp6.as_ref().map(|e| e.asn) || is_passive(b, &ib.name)
}

/// One way to a destination in the topology table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path6 {
    pub via: Ipv6Addr,
    pub iface: String,
    /// The distance through this neighbour, and the distance it reports.
    pub distance: u32,
    pub reported: u32,
    pub successor: bool,
}

/// A destination in the topology table: its feasible distance and the paths that pass the
/// feasibility condition (successors first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topo6 {
    pub net: Ipv6Net,
    pub feasible_distance: u32,
    pub paths: Vec<Path6>,
}

/// A route EIGRP for IPv6 installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eigrp6Route {
    pub net: Ipv6Net,
    pub via: Ipv6Addr,
    pub iface: String,
    pub metric: u32,
}

/// One EIGRP for IPv6 interface: its name, whether it is passive, its bandwidth (kbit/s) and
/// delay (tens of µs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iface6 {
    pub name: String,
    pub passive: bool,
    pub bandwidth: u32,
    pub delay: u32,
    /// Hello interval and the hold time it announces (seconds).
    pub hello_secs: u16,
    pub hold_secs: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Eigrp6State {
    pub router_ids: BTreeMap<DeviceId, Ipv4Addr>,
    pub interfaces: BTreeMap<DeviceId, Vec<Iface6>>,
    pub neighbors: BTreeMap<DeviceId, Vec<Neighbor6>>,
    pub topology: BTreeMap<DeviceId, Vec<Topo6>>,
    pub routes: BTreeMap<DeviceId, Vec<Eigrp6Route>>,
    /// Each router's best way to every prefix it knows: (slowest bandwidth, total delay, hops)
    /// and the interface of its successor (none for its own prefixes).
    pub best: BTreeMap<DeviceId, BTreeMap<Ipv6Net, ((u32, u64, u8), Option<String>)>>,
    /// Summaries a router advertises, with their metrics (each a route to Null0 here).
    pub summaries: BTreeMap<DeviceId, Vec<(Ipv6Net, u32)>>,
}

/// The router id: configured, else the highest IPv4 address (a loopback's first).
pub fn router_id(d: &Device) -> Option<Ipv4Addr> {
    let e = d.eigrp6.as_ref()?;
    e.router_id.or_else(|| crate::loopback::default_router_id(d))
}

/// Whether the process runs: configured, not shut down, on a router forwarding IPv6, with a
/// router id.
pub fn running(d: &Device) -> bool {
    d.eigrp6.as_ref().is_some_and(|e| !e.shutdown) && routes6(d) && router_id(d).is_some()
}

fn if_metrics(d: &Device, ifc: &L3If6) -> (u32, u32) {
    let (bw, delay) = default_metrics(&ifc.name);
    let port = (ifc.port != SVI_PORT).then(|| d.ports.get(ifc.port)).flatten();
    (port.and_then(|p| p.bandwidth).unwrap_or(bw).max(1), port.and_then(|p| p.delay).unwrap_or(delay))
}

/// The router's EIGRP for IPv6 interfaces: up, in its autonomous system.
fn ifaces(t: &Topology, d: &Device) -> Vec<(L3If6, Iface6)> {
    let Some(e) = &d.eigrp6 else { return Vec::new() };
    up_ifaces6(t, d)
        .into_iter()
        .filter(|i| conf6(d, i).and_then(|c| c.eigrp) == Some(e.asn))
        .map(|i| {
            let passive = is_passive(d, &i.name);
            let (bandwidth, delay) = if_metrics(d, &i);
            let (hello_secs, hold_secs) = timers(d, &i.name);
            let info = Iface6 { name: i.name.clone(), passive, bandwidth, delay, hello_secs, hold_secs };
            (i, info)
        })
        .collect()
}

/// A path's slowest bandwidth, total delay and hop count.
type Vector = (u32, u64, u8);

fn composite(v: Vector) -> u32 {
    metric(v.0, v.1)
}

/// A router's summaries for its autonomous system on an interface.
fn summaries_on(d: &Device, iface: &str) -> Vec<Ipv6Net> {
    let asn = d.eigrp6.as_ref().map_or(0, |e| e.asn);
    d.eigrp_summary.get(iface).map(|s| s.v6_for(asn)).unwrap_or_default()
}

/// A table as offered out of an interface with summaries: each summary that covers at least one
/// destination replaces them, with the best of their metrics.
fn summarize(table: &BTreeMap<Ipv6Net, Vector>, sums: &[Ipv6Net]) -> BTreeMap<Ipv6Net, Vector> {
    let mut out = table.clone();
    for s in sums {
        let covered: Vec<(Ipv6Net, Vector)> = out.iter().filter(|(n, _)| n.prefix >= s.prefix && s.contains(n.addr)).map(|(n, v)| (*n, *v)).collect();
        let Some(best) = covered.iter().map(|(_, v)| *v).min_by_key(|v| composite(*v)) else { continue };
        for (n, _) in &covered {
            out.remove(n);
        }
        out.insert(*s, best);
    }
    out
}

/// EIGRP for IPv6 converged over the lab as it is now, every neighbour heard.
pub fn compute(t: &Topology) -> Eigrp6State {
    compute_with(t, &[])
}

/// [`compute`] with neighbours that are no longer heard but still within their hold time.
pub fn compute_with(t: &Topology, held: &[Held]) -> Eigrp6State {
    let mut st = Eigrp6State::default();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| running(d)).collect();
    let mut ifs: BTreeMap<DeviceId, Vec<(L3If6, Iface6)>> = BTreeMap::new();
    for d in &routers {
        let Some(id) = router_id(d) else { continue };
        st.router_ids.insert(d.id, id);
        let list = ifaces(t, d);
        st.interfaces.insert(d.id, list.iter().map(|(_, i)| i.clone()).collect());
        ifs.insert(d.id, list);
    }
    // Adjacencies: (to, from's interface, its settings, to's link-local address, to's interface).
    let mut adj: BTreeMap<DeviceId, Vec<(DeviceId, L3If6, Iface6, Ipv6Addr, String)>> = BTreeMap::new();
    for a in &routers {
        let asn = a.eigrp6.as_ref().map_or(0, |e| e.asn);
        for (ia, info_a) in ifs.get(&a.id).into_iter().flatten().filter(|(_, x)| !x.passive) {
            let r = crate::ospf6::reach(t, a, ia);
            for b in routers.iter().filter(|b| b.id != a.id && b.eigrp6.as_ref().is_some_and(|e| e.asn == asn)) {
                for (ib, info_b) in ifs.get(&b.id).into_iter().flatten().filter(|(_, x)| !x.passive) {
                    let same_vlan = ib.vlan == ia.vlan || ia.port == SVI_PORT || ib.port == SVI_PORT;
                    if same_vlan && crate::ospf6::reaches(t, &r, b, ib) {
                        adj.entry(a.id).or_default().push((b.id, ia.clone(), info_a.clone(), ib.link_local, ib.name.clone()));
                        st.neighbors.entry(a.id).or_default().push(Neighbor6 {
                            device: b.id,
                            address: ib.link_local,
                            iface: ia.name.clone(),
                            hold_secs: info_b.hold_secs,
                            hello_secs: info_b.hello_secs,
                            held_until: None,
                        });
                    }
                }
            }
        }
    }
    // Neighbours still within their hold time: as if heard.
    for h in held {
        let Some((ia, info_a)) = ifs.get(&h.router).into_iter().flatten().find(|(i, x)| i.name == h.iface && !x.passive) else { continue };
        if adj.get(&h.router).is_some_and(|l| l.iter().any(|x| x.1.name == h.iface && x.3 == h.address)) {
            continue;
        }
        let (hello_secs, hold_secs) = t
            .device(h.peer)
            .and_then(|b| l3_ifaces6(b).into_iter().find(|i| i.link_local == h.address).map(|i| timers(b, &i.name)))
            .unwrap_or((HELLO_SECS, HOLD_SECS));
        adj.entry(h.router).or_default().push((h.peer, ia.clone(), info_a.clone(), h.address, h.peer_iface.clone()));
        st.neighbors.entry(h.router).or_default().push(Neighbor6 {
            device: h.peer,
            address: h.address,
            iface: h.iface.clone(),
            hold_secs,
            hello_secs,
            held_until: Some(h.until),
        });
    }
    // Connected prefixes each router advertises, with the interface's own metric.
    let connected: BTreeMap<DeviceId, BTreeMap<Ipv6Net, Vector>> = ifs
        .iter()
        .map(|(id, list)| {
            let mut m = BTreeMap::new();
            for (i, info) in list {
                for n in i.addresses.iter().map(|a| a.network()).filter(|n| !Ipv6Net::is_link_local(n.addr)) {
                    m.entry(n).or_insert((info.bandwidth, u64::from(info.delay), 0u8));
                }
            }
            (*id, m)
        })
        .collect();
    let dev = |id: DeviceId| routers.iter().find(|d| d.id == id).copied();
    // What neighbour `n` offers out of its interface `n_if`: its table, summarised there.
    let offered = |table: &BTreeMap<DeviceId, BTreeMap<Ipv6Net, Vector>>, n: DeviceId, n_if: &str| -> BTreeMap<Ipv6Net, Vector> {
        let own = table.get(&n).cloned().unwrap_or_default();
        match dev(n) {
            Some(d) => summarize(&own, &summaries_on(d, n_if)),
            None => own,
        }
    };
    // Distance vectors, round after round until nothing changes (every router's best path to
    // every prefix, each round one hop further).
    let mut table = connected.clone();
    for _ in 0..=routers.len().min(256) {
        let mut next = connected.clone();
        for (me, links) in &adj {
            let own: BTreeSet<Ipv6Net> = connected.get(me).map(|m| m.keys().copied().collect()).unwrap_or_default();
            let mine = next.entry(*me).or_default();
            for (n, _, info, _, n_if) in links {
                for (net, v) in &offered(&table, *n, n_if) {
                    if own.contains(net) {
                        continue;
                    }
                    let cand = (v.0.min(info.bandwidth), v.1.saturating_add(u64::from(info.delay)), v.2.saturating_add(1));
                    if mine.get(net).is_none_or(|old| composite(cand) < composite(*old)) {
                        mine.insert(*net, cand);
                    }
                }
            }
        }
        if next == table {
            break;
        }
        table = next;
    }
    // Each router's topology table and routes.
    for r in &routers {
        let own: BTreeSet<Ipv6Net> = connected.get(&r.id).map(|m| m.keys().copied().collect()).unwrap_or_default();
        let (max, variance) = r.eigrp6.as_ref().map_or((4, 1), |e| (e.max_paths(), u64::from(e.variance.unwrap_or(1).max(1))));
        let mut by_net: BTreeMap<Ipv6Net, Vec<Path6>> = BTreeMap::new();
        for (n, _, info, addr, n_if) in adj.get(&r.id).into_iter().flatten() {
            for (net, v) in offered(&table, *n, n_if).iter().filter(|(net, _)| !own.contains(*net)) {
                let through = (v.0.min(info.bandwidth), v.1.saturating_add(u64::from(info.delay)), v.2.saturating_add(1));
                by_net.entry(*net).or_default().push(Path6 {
                    via: *addr,
                    iface: info.name.clone(),
                    distance: composite(through),
                    reported: composite(*v),
                    successor: false,
                });
            }
        }
        let mut topo = Vec::new();
        let mut routes = Vec::new();
        let mut best = BTreeMap::new();
        for (net, v) in connected.get(&r.id).into_iter().flatten() {
            best.insert(*net, (*v, None));
        }
        for (net, mut paths) in by_net {
            let Some(fd) = paths.iter().map(|p| p.distance).min() else { continue };
            for p in &mut paths {
                p.successor = p.distance == fd;
            }
            // The feasibility condition: only successors and neighbours closer than we are.
            paths.retain(|p| p.successor || p.reported < fd);
            paths.sort_by_key(|p| (!p.successor, p.distance, p.via));
            // Successors, and with `variance` feasible successors within N × the distance.
            let limit = u64::from(fd).saturating_mul(variance);
            for p in paths.iter().filter(|p| p.successor || (variance > 1 && u64::from(p.distance) < limit)).take(max) {
                routes.push(Eigrp6Route { net, via: p.via, iface: p.iface.clone(), metric: p.distance });
            }
            if let (Some(v), Some(p)) = (table.get(&r.id).and_then(|m| m.get(&net)), paths.first()) {
                best.insert(net, (*v, Some(p.iface.clone())));
            }
            topo.push(Topo6 { net, feasible_distance: fd, paths });
        }
        // A summary with something under it is a route to nowhere here (Null0), so the
        // router never sends the summary's empty parts back out.
        let mut sums = Vec::new();
        for (i, _) in ifs.get(&r.id).into_iter().flatten() {
            for s in summaries_on(r, &i.name) {
                let under =
                    best.iter().filter(|(n, _)| n.prefix >= s.prefix && s.contains(n.addr) && **n != s).map(|(_, (v, _))| composite(*v)).min();
                if let Some(m) = under
                    && !sums.iter().any(|(x, _)| *x == s)
                {
                    sums.push((s, m));
                }
            }
        }
        st.summaries.insert(r.id, sums);
        st.best.insert(r.id, best);
        st.topology.insert(r.id, topo);
        st.routes.insert(r.id, routes);
    }
    st
}

/// A fingerprint of everything EIGRP for IPv6 depends on.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in &t.links {
        (l.a.device, l.a.port, l.b.device, l.b.port).hash(&mut h);
    }
    for d in t.devices.iter().filter(|d| matches!(d.kind, DeviceKind::Router | DeviceKind::Switch | DeviceKind::Hub)) {
        d.id.hash(&mut h);
        format!("{:?}{}{}", d.eigrp6, d.ipv6_routing, d.ip_routing).hash(&mut h);
        d.eigrp_summary.hash(&mut h);
        d.eigrp_if.hash(&mut h);
        for (i, p) in d.ports.iter().enumerate() {
            (p.shutdown, port_up(t, End { device: d.id, port: i }), format!("{:?}", p.ipv6), p.routed, p.ip, p.bandwidth, p.delay).hash(&mut h);
            for s in &p.subifs {
                (s.vlan, s.shutdown, format!("{:?}", s.ipv6)).hash(&mut h);
            }
            format!("{:?}", p.switchport).hash(&mut h);
        }
        for s in &d.svis {
            (s.vlan, s.shutdown, format!("{:?}", s.ipv6)).hash(&mut h);
        }
        format!("{:?}", d.loopbacks).hash(&mut h);
    }
    h.finish()
}

/// Most routes in one update, query or reply.
const ROUTES_PER_PACKET: usize = 32;

/// What a router advertises out of an interface: its connected prefixes and the routes it uses,
/// except those whose successor is out of that interface (split horizon), summarised as
/// configured there.
fn announced(st: &Eigrp6State, d: &Device, iface: &str) -> BTreeMap<Ipv6Net, InternalRoute6> {
    let Some(best) = st.best.get(&d.id) else { return BTreeMap::new() };
    let table: BTreeMap<Ipv6Net, Vector> = best.iter().filter(|(_, (_, out))| out.as_deref() != Some(iface)).map(|(n, (v, _))| (*n, *v)).collect();
    summarize(&table, &summaries_on(d, iface)).into_iter().map(|(n, v)| (n, InternalRoute6::new(n, v.0, v.1, v.2))).collect()
}

impl Sim {
    /// Brings EIGRP for IPv6's state up to date at the current simulated time: recomputes after
    /// the lab changed, keeps neighbours that fell silent until their hold time runs out, drops
    /// them when it does, logs neighbour changes and puts the resulting updates, queries and
    /// replies on the wire (as `eigrp_refresh` does for IPv4).
    pub(crate) fn eigrp6_refresh(&mut self, t: &Topology) {
        if !t.devices.iter().any(|d| d.eigrp6.is_some()) {
            self.eigrp6 = Some(Eigrp6State::default());
            self.eigrp6_held.clear();
            return;
        }
        let now = self.now;
        let f = fingerprint(t);
        let expired: Vec<Held> = self.eigrp6_held.iter().filter(|h| h.until <= now).cloned().collect();
        if self.eigrp6.is_some() && self.eigrp6_for == f && expired.is_empty() {
            return;
        }
        self.eigrp6_held.retain(|h| h.until > now);
        self.eigrp6_for = f;
        let old = self.eigrp6.take();
        let raw = compute(t);
        let heard = |st: &Eigrp6State, r: DeviceId, iface: &str, a: Ipv6Addr| {
            st.neighbors.get(&r).is_some_and(|l| l.iter().any(|n| n.iface == iface && n.address == a))
        };
        let iface_up =
            |st: &Eigrp6State, r: DeviceId, iface: &str| st.interfaces.get(&r).is_some_and(|l| l.iter().any(|i| i.name == iface && !i.passive));
        // Neighbours heard again, or whose interface here is down, are no longer held.
        self.eigrp6_held.retain(|h| !heard(&raw, h.router, &h.iface, h.address) && iface_up(&raw, h.router, &h.iface));
        let mut down: Vec<(DeviceId, Ipv6Addr, String, &'static str)> =
            expired.iter().map(|h| (h.router, h.address, h.iface.clone(), "hold time expired")).collect();
        if let Some(old) = &old {
            for (me, list) in &old.neighbors {
                let Some(d) = t.device(*me) else { continue };
                for n in list {
                    if heard(&raw, *me, &n.iface, n.address)
                        || self.eigrp6_held.iter().any(|x| x.router == *me && x.iface == n.iface && x.address == n.address)
                        || expired.iter().any(|x| x.router == *me && x.iface == n.iface && x.address == n.address)
                    {
                        continue;
                    }
                    if !iface_up(&raw, *me, &n.iface) || d.eigrp6.is_none() {
                        down.push((*me, n.address, n.iface.clone(), "interface down"));
                    } else if said_goodbye(t, d, n) {
                        down.push((*me, n.address, n.iface.clone(), "goodbye received"));
                    } else {
                        // Silent: kept from its last hello for the hold time it announced.
                        let hello = u64::from(n.hello_secs.max(1)) * 1_000_000;
                        let last = now - now % hello;
                        let until = last.saturating_add(u64::from(n.hold_secs) * 1_000_000);
                        let peer_iface = t
                            .device(n.device)
                            .and_then(|b| l3_ifaces6(b).into_iter().find(|i| i.link_local == n.address).map(|i| i.name))
                            .unwrap_or_default();
                        if until > now && self.eigrp6_held.len() < MAX_HELD {
                            self.eigrp6_held.push(Held {
                                router: *me,
                                iface: n.iface.clone(),
                                address: n.address,
                                peer: n.device,
                                peer_iface,
                                until,
                            });
                        } else {
                            down.push((*me, n.address, n.iface.clone(), "hold time expired"));
                        }
                    }
                }
            }
        }
        let new = if self.eigrp6_held.is_empty() { raw } else { compute_with(t, &self.eigrp6_held) };
        let asn_of = |id: DeviceId| t.device(id).and_then(|d| d.eigrp6.as_ref()).map_or(0, |e| e.asn);
        for (me, addr, iface, why) in down {
            if t.device(me).is_some_and(|d| d.eigrp6.is_some()) {
                self.log(me, &format!("EIGRP-IPv6 AS {}: neighbour {addr} ({iface}) is down: {why}", asn_of(me)));
            }
        }
        if let Some(old) = &old {
            for (me, list) in &new.neighbors {
                for n in
                    list.iter().filter(|n| !old.neighbors.get(me).is_some_and(|o| o.iter().any(|x| x.iface == n.iface && x.address == n.address)))
                {
                    self.log(*me, &format!("EIGRP-IPv6 AS {}: neighbour {} ({}) is up: new adjacency", asn_of(*me), n.address, n.iface));
                }
            }
            self.eigrp6_packets(t, old, &new);
        }
        self.eigrp6 = Some(new);
    }

    /// When the next held neighbour's hold time runs out.
    pub(crate) fn eigrp6_next_expiry(&self) -> Option<u64> {
        self.eigrp6_held.iter().map(|h| h.until).min()
    }

    /// The packets DUAL sends going from `old` to `new` (as for IPv4, `eigrp`): the whole table
    /// to each new neighbour (an initial update, unicast to its link-local address); a query
    /// for each destination a router lost altogether, answered by every neighbour there; an
    /// update for each destination announced differently out of an interface (withdrawn ones
    /// as unreachable). Each is acknowledged. They go to `FF02::A` or unicast, from link-local
    /// addresses.
    pub(crate) fn eigrp6_packets(&mut self, t: &Topology, old: &Eigrp6State, new: &Eigrp6State) {
        type Out = (u64, DeviceId, String, Option<Ipv6Addr>, u8, u32, Vec<InternalRoute6>);
        let at = self.now;
        let mut out: Vec<Out> = Vec::new();
        for (me, ifs) in &new.interfaces {
            let Some(d) = t.device(*me) else { continue };
            let nbrs: Vec<Neighbor6> = new.neighbors.get(me).into_iter().flatten().filter(|n| n.held_until.is_none()).cloned().collect();
            let known = |st: &Eigrp6State, net: &Ipv6Net| st.best.get(me).is_some_and(|b| b.contains_key(net));
            for ifc in ifs.iter().filter(|i| !i.passive) {
                let here: Vec<&Neighbor6> = nbrs.iter().filter(|n| n.iface == ifc.name).collect();
                if here.is_empty() {
                    continue;
                }
                let after = announced(new, d, &ifc.name);
                let before = if old.interfaces.get(me).is_some_and(|l| l.iter().any(|i| i.name == ifc.name)) {
                    announced(old, d, &ifc.name)
                } else {
                    BTreeMap::new()
                };
                for n in &here {
                    if !old.neighbors.get(me).is_some_and(|o| o.iter().any(|x| x.iface == n.iface && x.address == n.address)) {
                        out.push((
                            1_000,
                            *me,
                            ifc.name.clone(),
                            Some(n.address),
                            OPCODE_UPDATE,
                            FLAG_INIT | FLAG_EOT,
                            after.values().copied().collect(),
                        ));
                    }
                }
                let lost: Vec<Ipv6Net> = before.keys().filter(|n| !after.contains_key(n) && !known(new, n) && known(old, n)).copied().collect();
                let mut changed: Vec<InternalRoute6> = after.iter().filter(|(n, r)| before.get(n).is_none_or(|b| b != *r)).map(|(_, r)| *r).collect();
                changed.extend(before.keys().filter(|n| !after.contains_key(n) && !lost.contains(n)).map(|n| InternalRoute6::unreachable(*n)));
                if !lost.is_empty() {
                    out.push((2_000, *me, ifc.name.clone(), None, OPCODE_QUERY, 0, lost.iter().map(|n| InternalRoute6::unreachable(*n)).collect()));
                    // Every neighbour there answers with what it has for them.
                    for n in &here {
                        let Some(b) = t.device(n.device) else { continue };
                        let Some(back) = new.neighbors.get(&n.device).and_then(|l| l.iter().find(|x| x.device == *me && x.held_until.is_none()))
                        else {
                            continue;
                        };
                        let theirs = announced(new, b, &back.iface);
                        let routes = lost.iter().map(|net| theirs.get(net).copied().unwrap_or(InternalRoute6::unreachable(*net))).collect();
                        out.push((3_000, n.device, back.iface.clone(), Some(back.address), OPCODE_REPLY, 0, routes));
                    }
                }
                if !changed.is_empty() {
                    out.push((4_000, *me, ifc.name.clone(), None, OPCODE_UPDATE, 0, changed));
                }
            }
        }
        for (offset, dev, iface, to, opcode, flags, routes) in out {
            let Some(d) = t.device(dev) else { continue };
            let asn = d.eigrp6.as_ref().map_or(0, |e| e.asn);
            let Some(ifc) = up_ifaces6(t, d).into_iter().find(|i| i.name == iface) else { continue };
            let chunks: Vec<&[InternalRoute6]> = if routes.is_empty() { vec![&[]] } else { routes.chunks(ROUTES_PER_PACKET).take(64).collect() };
            for (k, chunk) in (0u64..).zip(chunks) {
                let seq = self.eigrp_seq(dev);
                let p = EigrpPacket { opcode, flags, seq, ack: 0, asn, tlvs: chunk.iter().map(|r| Tlv::Internal6(*r)).collect() };
                let when = at.saturating_add(offset).saturating_add(k * 100);
                self.eigrp6_send(t, d, &ifc, to, p.encode(), when);
                // Each receiver acknowledges, unicast.
                let receivers: Vec<(DeviceId, Ipv6Addr)> = new
                    .neighbors
                    .get(&dev)
                    .into_iter()
                    .flatten()
                    .filter(|n| n.iface == iface && n.held_until.is_none() && to.is_none_or(|a| a == n.address))
                    .map(|n| (n.device, n.address))
                    .collect();
                for (rd, raddr) in receivers {
                    let Some(r) = t.device(rd) else { continue };
                    let Some(rif) = up_ifaces6(t, r).into_iter().find(|i| i.link_local == raddr) else { continue };
                    let ack = EigrpPacket::ack(asn, seq).encode();
                    self.eigrp6_send(t, r, &rif, Some(ifc.link_local), ack, when.saturating_add(500));
                }
            }
        }
    }

    /// Queues an EIGRP for IPv6 packet out of an interface: to `FF02::A`, or unicast to a
    /// neighbour's link-local address.
    fn eigrp6_send(&mut self, t: &Topology, d: &Device, ifc: &L3If6, to: Option<Ipv6Addr>, payload: Vec<u8>, at: u64) {
        use netlabcraft_wire::Frame;
        use netlabcraft_wire::ipv6::{ETHERTYPE_IPV6, Ipv6, multicast_mac};
        let (dst, mac) = match to {
            None => (EIGRP_ROUTERS6, multicast_mac(EIGRP_ROUTERS6)),
            Some(a) => {
                let mac = t.devices.iter().find_map(|x| up_ifaces6(t, x).into_iter().find(|i| i.link_local == a).map(|i| i.mac));
                (a, mac.unwrap_or(multicast_mac(EIGRP_ROUTERS6)))
            }
        };
        let ip = Ipv6 { hop_limit: 1, next_header: PROTO_EIGRP, src: ifc.link_local, dst, payload };
        let frame = Frame { dst: mac, src: ifc.mac, vlan: ifc.vlan, ethertype: ETHERTYPE_IPV6, payload: ip.encode() };
        self.queue_frame(t, d, ifc.port, frame, at);
    }
}
