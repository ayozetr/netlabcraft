//! OSPF (RFC 2328), single area, computed the way the protocol converges.
//!
//! Interfaces whose address matches a `network … area` statement run OSPF. Two routers are
//! neighbours when such interfaces (not passive) share a subnet and reach each other at layer 2.
//! Every router runs SPF (Dijkstra) over the neighbour graph with interface costs (reference
//! bandwidth 100 Mbit/s: Fast and Gigabit Ethernet cost 1) and installs the networks other routers
//! advertise. A router with `default-information originate` and a static default route
//! advertises it (type 2 external, metric 1). Hello/dead timers and LSAs are not simulated yet.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use packetcraft_core::{DeviceId, Ipv4Net};
use packetcraft_model::{Device, DeviceKind, End, NextHop, Topology};

use crate::{L3If, port_up, up_ifaces};

/// A route OSPF installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfRoute {
    pub net: Ipv4Net,
    pub via: Ipv4Addr,
    pub iface: String,
    pub cost: u32,
    /// A type-2 external route (the default route).
    pub external: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub device: DeviceId,
    pub router_id: Ipv4Addr,
    pub address: Ipv4Addr,
    /// This router's interface towards it.
    pub iface: String,
}

/// OSPF's view of the whole lab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OspfState {
    pub router_ids: BTreeMap<DeviceId, Ipv4Addr>,
    pub neighbors: BTreeMap<DeviceId, Vec<Neighbor>>,
    pub routes: BTreeMap<DeviceId, Vec<OspfRoute>>,
    /// Interfaces running OSPF, per router: (name, network, cost, passive).
    pub interfaces: BTreeMap<DeviceId, Vec<(String, Ipv4Net, u32, bool)>>,
}

pub fn iface_cost(name: &str) -> u32 {
    if name.starts_with("GigabitEthernet") || name.starts_with("FastEthernet") {
        1
    } else if name.starts_with("Serial") {
        64
    } else {
        10
    }
}

/// The interfaces of a router OSPF runs on.
fn ospf_ifaces(t: &Topology, d: &Device) -> Vec<(L3If, bool)> {
    let Some(o) = &d.ospf else { return Vec::new() };
    up_ifaces(t, d)
        .into_iter()
        .filter(|i| o.networks.iter().any(|n| n.matches(i.net.addr)))
        .map(|i| {
            let passive = o.passive.iter().any(|p| packetcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == i.name));
            (i, passive)
        })
        .collect()
}

/// The router id: configured, else the highest interface address.
pub fn router_id(d: &Device) -> Option<Ipv4Addr> {
    let o = d.ospf.as_ref()?;
    o.router_id.or_else(|| crate::l3_ifaces(d).iter().map(|i| i.net.addr).max())
}

/// Ports reachable at layer 2 from a port (through cables, switches and hubs).
pub(crate) fn l2_reach(t: &Topology, from: End) -> BTreeSet<End> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![from];
    while let Some(e) = todo.pop() {
        if !port_up(t, e) {
            continue;
        }
        let Some(peer) = t.peer(e) else { continue };
        if !seen.insert(peer) {
            continue;
        }
        if let Some(pd) = t.device(peer.device)
            && matches!(pd.kind, DeviceKind::Switch | DeviceKind::Hub)
        {
            for p in 0..pd.ports.len() {
                let next = End { device: pd.id, port: p };
                if p != peer.port && seen.insert(next) {
                    todo.push(next);
                }
            }
        }
        if seen.len() > 10_000 {
            break;
        }
    }
    seen
}

pub fn compute(t: &Topology) -> OspfState {
    let mut st = OspfState::default();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| d.kind == DeviceKind::Router && d.ospf.is_some()).collect();
    let mut ifs: BTreeMap<DeviceId, Vec<(L3If, bool)>> = BTreeMap::new();
    for d in &routers {
        if let Some(id) = router_id(d) {
            st.router_ids.insert(d.id, id);
        }
        let list = ospf_ifaces(t, d);
        st.interfaces.insert(d.id, list.iter().map(|(i, p)| (i.name.clone(), i.net.network(), iface_cost(&i.name), *p)).collect());
        ifs.insert(d.id, list);
    }
    // Adjacencies: (from, to, from's interface, to's address, cost).
    let mut adj: BTreeMap<DeviceId, Vec<(DeviceId, L3If, Ipv4Addr, u32)>> = BTreeMap::new();
    for a in &routers {
        for (ia, pa) in ifs.get(&a.id).into_iter().flatten() {
            if *pa {
                continue;
            }
            let reach = l2_reach(t, End { device: a.id, port: ia.port });
            for b in &routers {
                if b.id == a.id {
                    continue;
                }
                for (ib, pb) in ifs.get(&b.id).into_iter().flatten() {
                    let same = ib.net.network() == ia.net.network() && ib.vlan == ia.vlan;
                    if !*pb && same && reach.contains(&End { device: b.id, port: ib.port }) {
                        adj.entry(a.id).or_default().push((b.id, ia.clone(), ib.net.addr, iface_cost(&ia.name)));
                        if let Some(rid) = st.router_ids.get(&b.id) {
                            st.neighbors.entry(a.id).or_default().push(Neighbor {
                                device: b.id,
                                router_id: *rid,
                                address: ib.net.addr,
                                iface: ia.name.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    // Networks each router advertises (cost of its own interface added).
    let stubs: BTreeMap<DeviceId, Vec<(Ipv4Net, u32)>> =
        ifs.iter().map(|(d, l)| (*d, l.iter().map(|(i, _)| (i.net.network(), iface_cost(&i.name))).collect())).collect();
    let originators: Vec<DeviceId> = routers
        .iter()
        .filter(|d| {
            d.ospf.as_ref().is_some_and(|o| o.default_originate)
                && d.routes.iter().any(|r| r.net.prefix == 0 && matches!(r.via, NextHop::Address(_) | NextHop::Interface(_)))
        })
        .map(|d| d.id)
        .collect();
    for r in &routers {
        // Dijkstra: (cost, first hop interface, first hop address) per router.
        let mut best: BTreeMap<DeviceId, (u32, Option<(L3If, Ipv4Addr)>)> = BTreeMap::new();
        best.insert(r.id, (0, None));
        let mut done: BTreeSet<DeviceId> = BTreeSet::new();
        loop {
            let next = best
                .iter()
                .filter(|(d, _)| !done.contains(*d))
                .min_by_key(|(d, (c, _))| (*c, st.router_ids.get(*d).copied()))
                .map(|(d, v)| (*d, v.clone()));
            let Some((u, (cu, hop))) = next else { break };
            done.insert(u);
            for (v, ifc, addr, cost) in adj.get(&u).into_iter().flatten() {
                let c = cu.saturating_add(*cost);
                let first = hop.clone().or_else(|| Some((ifc.clone(), *addr)));
                let better = best.get(v).is_none_or(|(old, oh)| c < *old || (c == *old && first.as_ref().map(|f| f.1) < oh.as_ref().map(|f| f.1)));
                if better && !done.contains(v) {
                    best.insert(*v, (c, first));
                }
            }
        }
        let mine: Vec<Ipv4Net> = up_ifaces(t, r).iter().map(|i| i.net.network()).collect();
        let mut routes: BTreeMap<Ipv4Net, OspfRoute> = BTreeMap::new();
        for (x, (cx, hop)) in &best {
            let Some((ifc, via)) = hop else { continue };
            for (net, c) in stubs.get(x).into_iter().flatten() {
                if mine.contains(net) {
                    continue;
                }
                let cand = OspfRoute { net: *net, via: *via, iface: ifc.name.clone(), cost: cx.saturating_add(*c), external: false };
                if routes.get(net).is_none_or(|old| (cand.cost, cand.via) < (old.cost, old.via)) {
                    routes.insert(*net, cand);
                }
            }
            if originators.contains(x) && *x != r.id {
                let def = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0);
                let cand = OspfRoute { net: def, via: *via, iface: ifc.name.clone(), cost: 1, external: true };
                if routes.get(&def).is_none_or(|old| (*cx, cand.via) < (old.cost.saturating_add(*cx), old.via)) {
                    routes.insert(def, cand);
                }
            }
        }
        st.routes.insert(r.id, routes.into_values().collect());
    }
    st
}

/// A fingerprint of everything OSPF depends on.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in &t.links {
        (l.a.device, l.a.port, l.b.device, l.b.port).hash(&mut h);
    }
    for d in &t.devices {
        d.id.hash(&mut h);
        for p in &d.ports {
            (p.shutdown, p.ip).hash(&mut h);
            for s in &p.subifs {
                (s.ip, s.vlan, s.shutdown).hash(&mut h);
            }
        }
        if d.kind == DeviceKind::Router {
            format!("{:?}{:?}{:?}", d.ospf, d.rip, d.routes).hash(&mut h);
        }
    }
    h.finish()
}
