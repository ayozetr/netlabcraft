//! EIGRP (RFC 7868) for IPv4, computed the way DUAL converges.
//!
//! Interfaces whose address falls under a `network` statement run EIGRP. Two routers are
//! neighbours when such interfaces (not passive) share a subnet, reach each other at layer 2 and
//! belong to the same autonomous system. Each router starts from its EIGRP networks and, round
//! after round, takes every route its neighbours use, adding the link it hears it on: the path's
//! bandwidth is the slowest link's, its delay the sum of the links' delays, and the composite
//! metric (K1 = K3 = 1) is `256 × (10⁷ / bandwidth in kbit/s + delay in tens of µs)` (RFC 7868
//! §5.6.2). The lowest is the feasible distance and its neighbour the successor; any other
//! neighbour whose reported distance is below the feasible distance is a feasible successor
//! (the feasibility condition, §5.3). Split horizon: a router doesn't offer a route back to the
//! neighbour it uses for it. Hello and hold timers, queries and stuck-in-active are not simulated.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use packetcraft_core::{DeviceId, Ipv4Net};
use packetcraft_model::{Device, Topology};

use crate::{L3If, up_ifaces};

/// Administrative distance of internal EIGRP routes.
pub const DISTANCE: u8 = 90;
/// Hold time announced in hellos (seconds).
pub const HOLD_SECS: u16 = packetcraft_wire::eigrp::HOLD_SECS;
/// The metric that means unreachable.
pub const INFINITY: u32 = u32::MAX;

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

/// An EIGRP interface of a router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EigrpIf {
    pub name: String,
    pub net: Ipv4Net,
    pub bandwidth: u32,
    pub delay: u32,
    pub passive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub device: DeviceId,
    pub address: Ipv4Addr,
    /// This router's interface towards it.
    pub iface: String,
}

/// A route EIGRP installs (the successor's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EigrpRoute {
    pub net: Ipv4Net,
    pub via: Ipv4Addr,
    pub iface: String,
    /// The feasible distance.
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
}

/// The router id: configured, else the highest interface address.
pub fn router_id(d: &Device) -> Option<Ipv4Addr> {
    let e = d.eigrp.as_ref()?;
    e.router_id.or_else(|| crate::l3_ifaces(d).iter().map(|i| i.net.addr).max())
}

fn eigrp_ifaces(t: &Topology, d: &Device) -> Vec<(L3If, bool)> {
    let Some(e) = &d.eigrp else { return Vec::new() };
    up_ifaces(t, d)
        .into_iter()
        .filter(|i| e.covers(i.net.addr))
        .map(|i| {
            let passive = e.passive.iter().any(|p| packetcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == i.name));
            (i, passive)
        })
        .collect()
}

/// A router's best way to a network while computing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Best {
    bw: u32,
    delay: u64,
    metric: u32,
    /// (our interface, neighbour's address, neighbour) unless connected.
    via: Option<(String, Ipv4Addr, DeviceId)>,
}

/// Who hears whom: (our interface, its bandwidth and delay, their address, their device).
type Adj = (L3If, u32, u32, Ipv4Addr, DeviceId);

/// Rounds of updates at most (each round carries routes one router further).
const MAX_ROUNDS: usize = 128;

pub fn compute(t: &Topology) -> EigrpState {
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
                    EigrpIf { name: i.name.clone(), net: i.net.network(), bandwidth, delay, passive: *p }
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
                        adj.entry(a.id).or_default().push((ia.clone(), bw, delay, ib.net.addr, b.id));
                        st.neighbors.entry(a.id).or_default().push(Neighbor { device: b.id, address: ib.net.addr, iface: ia.name.clone() });
                    }
                }
            }
        }
    }
    // Each router's own networks.
    let mut connected: BTreeMap<DeviceId, BTreeMap<Ipv4Net, Best>> = BTreeMap::new();
    for d in &routers {
        let mut tbl: BTreeMap<Ipv4Net, Best> = BTreeMap::new();
        for (i, _) in ifs.get(&d.id).into_iter().flatten() {
            let (bw, delay) = if_metrics(d, i);
            let b = Best { bw, delay: u64::from(delay), metric: metric(bw, u64::from(delay)), via: None };
            if tbl.get(&i.net.network()).is_none_or(|old| b.metric < old.metric) {
                tbl.insert(i.net.network(), b);
            }
        }
        connected.insert(d.id, tbl);
    }
    let offer = |from: &BTreeMap<Ipv4Net, Best>, me: DeviceId, (_, bw, delay, ..): &Adj| -> Vec<(Ipv4Net, Best, u32)> {
        from.iter()
            // Split horizon: nothing offered back to the neighbour it goes through.
            .filter(|(_, b)| !b.via.as_ref().is_some_and(|(_, _, dev)| *dev == me))
            .map(|(net, b)| {
                let nbw = b.bw.min(*bw);
                let nd = b.delay.saturating_add(u64::from(*delay));
                (*net, Best { bw: nbw, delay: nd, metric: metric(nbw, nd), via: None }, b.metric)
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
    // The topology table: successors and feasible successors.
    for d in &routers {
        let Some(tbl) = tables.get(&d.id) else { continue };
        let mut entries = Vec::new();
        let mut routes = Vec::new();
        for (net, best) in tbl {
            let fd = best.metric;
            let mut paths = Vec::new();
            match &best.via {
                None => {
                    let name =
                        ifs.get(&d.id).into_iter().flatten().find(|(i, _)| i.net.network() == *net).map_or(String::new(), |(i, _)| i.name.clone());
                    paths.push(Path { via: None, iface: name, distance: fd, reported: 0, successor: true });
                }
                Some((iface, addr, _)) => {
                    routes.push(EigrpRoute { net: *net, via: *addr, iface: iface.clone(), metric: fd });
                    for a in adj.get(&d.id).into_iter().flatten() {
                        let Some(from) = tables.get(&a.4) else { continue };
                        for (n, cand, reported) in offer(from, d.id, a) {
                            if n != *net {
                                continue;
                            }
                            let successor = cand.metric == fd;
                            if successor || reported < fd {
                                paths.push(Path { via: Some(a.3), iface: a.0.name.clone(), distance: cand.metric, reported, successor });
                            }
                        }
                    }
                    // The successor in use first, then the others by distance.
                    paths.sort_by_key(|p| (p.via != Some(*addr), !p.successor, p.distance, p.via));
                }
            }
            entries.push(TopoEntry { net: *net, fd, paths });
        }
        st.topology.insert(d.id, entries);
        st.routes.insert(d.id, routes);
    }
    st
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
