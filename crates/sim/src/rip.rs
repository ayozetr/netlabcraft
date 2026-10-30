//! RIP (RFC 2453), computed the way the protocol converges.
//!
//! Interfaces under a `network` statement (classful match) run RIP. Two routers are neighbours
//! when such interfaces (not passive) share a subnet and reach each other at layer 2. Each
//! router starts from its connected networks (0 hops) and, round after round, takes from each
//! neighbour every route one hop further, keeping the fewest hops (ties: lowest next hop) and
//! dropping anything past 15. Split horizon: a router never offers a route back to the neighbour
//! it learned it from. A router with `default-information originate` advertises a default route.
//! Both versions behave classless (no automatic summarisation); update timers are not simulated.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use packetcraft_core::{DeviceId, Ipv4Net};
use packetcraft_model::{Device, DeviceKind, End, Topology};

use crate::{L3If, up_ifaces};

/// Hop count meaning unreachable.
pub const INFINITY: u8 = 16;

/// A route RIP installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RipRoute {
    pub net: Ipv4Net,
    pub via: Ipv4Addr,
    pub iface: String,
    pub hops: u8,
}

/// RIP's view of the lab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RipState {
    pub routes: BTreeMap<DeviceId, Vec<RipRoute>>,
    /// Neighbours per router: (their device, their address, our interface).
    pub neighbors: BTreeMap<DeviceId, Vec<(DeviceId, Ipv4Addr, String)>>,
    /// Interfaces running RIP per router: (name, network, passive).
    pub interfaces: BTreeMap<DeviceId, Vec<(String, Ipv4Net, bool)>>,
}

fn rip_ifaces(t: &Topology, d: &Device) -> Vec<(L3If, bool)> {
    let Some(r) = &d.rip else { return Vec::new() };
    up_ifaces(t, d)
        .into_iter()
        .filter(|i| r.covers(i.net.addr))
        .map(|i| {
            let passive = r.passive.iter().any(|p| packetcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == i.name));
            (i, passive)
        })
        .collect()
}

/// One entry of a router's table while computing: hops, and (interface, next hop) unless connected.
type Entry = (u8, Option<(L3If, Ipv4Addr, DeviceId)>);

pub fn compute(t: &Topology) -> RipState {
    let mut st = RipState::default();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| d.routes_ip() && d.kind == DeviceKind::Router && d.rip.is_some()).collect();
    let mut ifs: BTreeMap<DeviceId, Vec<(L3If, bool)>> = BTreeMap::new();
    for d in &routers {
        let list = rip_ifaces(t, d);
        st.interfaces.insert(d.id, list.iter().map(|(i, p)| (i.name.clone(), i.net.network(), *p)).collect());
        ifs.insert(d.id, list);
    }
    // Who hears whom: (router, our interface, their address, their device).
    let mut adj: BTreeMap<DeviceId, Vec<(L3If, Ipv4Addr, DeviceId)>> = BTreeMap::new();
    for a in &routers {
        for (ia, pa) in ifs.get(&a.id).into_iter().flatten() {
            if *pa {
                continue;
            }
            let reach = crate::ospf::l2_reach(t, End { device: a.id, port: ia.port });
            for b in &routers {
                if b.id == a.id {
                    continue;
                }
                for (ib, pb) in ifs.get(&b.id).into_iter().flatten() {
                    if !*pb
                        && ib.net.network() == ia.net.network()
                        && (ib.vlan == ia.vlan || ia.port == crate::SVI_PORT || ib.port == crate::SVI_PORT)
                        && reach.contains(&End { device: b.id, port: ib.port })
                    {
                        adj.entry(a.id).or_default().push((ia.clone(), ib.net.addr, b.id));
                        st.neighbors.entry(a.id).or_default().push((b.id, ib.net.addr, ia.name.clone()));
                    }
                }
            }
        }
    }
    // Start: connected networks, and the default route where originated.
    let mut tables: BTreeMap<DeviceId, BTreeMap<Ipv4Net, Entry>> = BTreeMap::new();
    for d in &routers {
        let mut tbl: BTreeMap<Ipv4Net, Entry> = BTreeMap::new();
        for (i, _) in ifs.get(&d.id).into_iter().flatten() {
            tbl.insert(i.net.network(), (0, None));
        }
        let originate = d.rip.as_ref().is_some_and(|r| r.default_originate) && d.routes.iter().any(|r| r.net.prefix == 0);
        if originate {
            tbl.insert(Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0), (0, None));
        }
        tables.insert(d.id, tbl);
    }
    // Rounds of updates until nothing changes (at most 16: hop counts stop there).
    for _ in 0..=INFINITY {
        let snapshot = tables.clone();
        let mut changed = false;
        for d in &routers {
            let Some(tbl) = tables.get_mut(&d.id) else { continue };
            for (ifc, addr, from) in adj.get(&d.id).into_iter().flatten() {
                for (net, (hops, learned)) in snapshot.get(from).into_iter().flatten() {
                    // Split horizon.
                    if learned.as_ref().is_some_and(|(_, _, back)| *back == d.id) {
                        continue;
                    }
                    let h = hops.saturating_add(1);
                    if h >= INFINITY {
                        continue;
                    }
                    let cand: Entry = (h, Some((ifc.clone(), *addr, *from)));
                    let better = match tbl.get(net) {
                        None => true,
                        Some((old, None)) => h < *old,
                        Some((old, Some((_, oa, _)))) => h < *old || (h == *old && *addr < *oa),
                    };
                    if better {
                        tbl.insert(*net, cand);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    for (id, tbl) in tables {
        let routes =
            tbl.into_iter().filter_map(|(net, (hops, via))| via.map(|(ifc, addr, _)| RipRoute { net, via: addr, iface: ifc.name, hops })).collect();
        st.routes.insert(id, routes);
    }
    st
}
