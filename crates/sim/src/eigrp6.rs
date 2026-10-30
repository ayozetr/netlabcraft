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
//! `maximum-paths` successors are installed. Hellos go to `FF02::A` every 5 s (`control`).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use netlabcraft_core::{DeviceId, Ipv6Net};
use netlabcraft_model::{Device, DeviceKind, End, Topology};

use crate::eigrp::{default_metrics, metric};
use crate::v6::{L3If6, conf6, routes6, up_ifaces6};
use crate::{SVI_PORT, port_up};

/// A neighbour heard on one interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor6 {
    pub device: DeviceId,
    /// Their link-local address.
    pub address: Ipv6Addr,
    /// Our interface towards them.
    pub iface: String,
    pub hold_secs: u16,
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Eigrp6State {
    pub router_ids: BTreeMap<DeviceId, Ipv4Addr>,
    pub interfaces: BTreeMap<DeviceId, Vec<Iface6>>,
    pub neighbors: BTreeMap<DeviceId, Vec<Neighbor6>>,
    pub topology: BTreeMap<DeviceId, Vec<Topo6>>,
    pub routes: BTreeMap<DeviceId, Vec<Eigrp6Route>>,
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
            let passive = e.passive.iter().any(|p| netlabcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == i.name));
            let (bandwidth, delay) = if_metrics(d, &i);
            let info = Iface6 { name: i.name.clone(), passive, bandwidth, delay };
            (i, info)
        })
        .collect()
}

/// A path's slowest bandwidth, total delay and hop count.
type Vector = (u32, u64, u8);

fn composite(v: Vector) -> u32 {
    metric(v.0, v.1)
}

pub fn compute(t: &Topology) -> Eigrp6State {
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
    // Adjacencies: (from, to, from's interface, to's link-local address).
    let mut adj: BTreeMap<DeviceId, Vec<(DeviceId, L3If6, Iface6, Ipv6Addr)>> = BTreeMap::new();
    for a in &routers {
        let asn = a.eigrp6.as_ref().map_or(0, |e| e.asn);
        for (ia, info_a) in ifs.get(&a.id).into_iter().flatten().filter(|(_, x)| !x.passive) {
            let r = crate::ospf6::reach(t, a, ia);
            for b in routers.iter().filter(|b| b.id != a.id && b.eigrp6.as_ref().is_some_and(|e| e.asn == asn)) {
                for (ib, _) in ifs.get(&b.id).into_iter().flatten().filter(|(_, x)| !x.passive) {
                    let same_vlan = ib.vlan == ia.vlan || ia.port == SVI_PORT || ib.port == SVI_PORT;
                    if same_vlan && crate::ospf6::reaches(t, &r, b, ib) {
                        adj.entry(a.id).or_default().push((b.id, ia.clone(), info_a.clone(), ib.link_local));
                        st.neighbors.entry(a.id).or_default().push(Neighbor6 {
                            device: b.id,
                            address: ib.link_local,
                            iface: ia.name.clone(),
                            hold_secs: netlabcraft_wire::eigrp::HOLD_SECS,
                        });
                    }
                }
            }
        }
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
    // Distance vectors, round after round until nothing changes (every router's best path to
    // every prefix, each round one hop further).
    let mut table = connected.clone();
    for _ in 0..=routers.len().min(256) {
        let mut next = connected.clone();
        for (me, links) in &adj {
            let own: BTreeSet<Ipv6Net> = connected.get(me).map(|m| m.keys().copied().collect()).unwrap_or_default();
            let mine = next.entry(*me).or_default();
            for (n, _, info, _) in links {
                for (net, v) in table.get(n).into_iter().flatten() {
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
        let max = r.eigrp6.as_ref().map_or(4, |e| e.max_paths());
        let mut by_net: BTreeMap<Ipv6Net, Vec<Path6>> = BTreeMap::new();
        for (n, _, info, addr) in adj.get(&r.id).into_iter().flatten() {
            for (net, v) in table.get(n).into_iter().flatten().filter(|(net, _)| !own.contains(*net)) {
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
        for (net, mut paths) in by_net {
            let Some(fd) = paths.iter().map(|p| p.distance).min() else { continue };
            for p in &mut paths {
                p.successor = p.distance == fd;
            }
            // The feasibility condition: only successors and neighbours closer than we are.
            paths.retain(|p| p.successor || p.reported < fd);
            paths.sort_by_key(|p| (!p.successor, p.distance, p.via));
            for p in paths.iter().filter(|p| p.successor).take(max) {
                routes.push(Eigrp6Route { net, via: p.via, iface: p.iface.clone(), metric: p.distance });
            }
            topo.push(Topo6 { net, feasible_distance: fd, paths });
        }
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
