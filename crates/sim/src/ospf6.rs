//! OSPFv3 (RFC 5340): OSPF for IPv6, computed from the topology like OSPFv2.
//!
//! Interfaces join a process with `ipv6 ospf N area A`; neighbours meet over link-local
//! addresses on a shared link (their global prefixes don't have to match), and routes lead to
//! the neighbour's link-local address. Hellos go on the wire every 10 s (`control`).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use packetcraft_core::{DeviceId, Ipv6Net};
use packetcraft_model::{Device, DeviceKind, End, Topology};

use crate::v6::{L3If6, conf6, routes6, up_ifaces6};
use crate::{SVI_PORT, port_up};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor6 {
    pub device: DeviceId,
    pub router_id: Ipv4Addr,
    /// Their link-local address.
    pub address: Ipv6Addr,
    /// Our interface towards them.
    pub iface: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ospf6Route {
    pub net: Ipv6Net,
    pub via: Ipv6Addr,
    pub iface: String,
    pub cost: u32,
    pub inter_area: bool,
    pub external: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ospf6State {
    pub router_ids: BTreeMap<DeviceId, Ipv4Addr>,
    pub neighbors: BTreeMap<DeviceId, Vec<Neighbor6>>,
    pub routes: BTreeMap<DeviceId, Vec<Ospf6Route>>,
    /// Per router: (interface, area, cost, passive).
    pub interfaces: BTreeMap<DeviceId, Vec<(String, u32, u32, bool)>>,
}

/// The router ID: configured, else the highest IPv4 interface address. OSPFv3 doesn't start
/// without one.
pub fn router_id(d: &Device) -> Option<Ipv4Addr> {
    let o = d.ospf6.as_ref()?;
    o.router_id.or_else(|| crate::l3_ifaces(d).iter().map(|i| i.net.addr).max())
}

fn cost(ifc: &L3If6) -> u32 {
    if ifc.name.starts_with("Serial") { 64 } else { 1 }
}

/// The OSPFv3 interfaces of a router: up, IPv6 on, in its process; with their area and
/// whether they are passive.
fn ifaces(t: &Topology, d: &Device) -> Vec<(L3If6, u32, bool)> {
    let Some(o) = &d.ospf6 else { return Vec::new() };
    up_ifaces6(t, d)
        .into_iter()
        .filter_map(|i| {
            let (process, area) = conf6(d, &i)?.ospf?;
            let passive = o.passive.iter().any(|p| packetcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == i.name));
            (process == o.process).then_some((i, area, passive))
        })
        .collect()
}

/// Ports reachable at layer 2 from an interface.
fn reach(t: &Topology, d: &Device, ifc: &L3If6) -> BTreeSet<End> {
    if ifc.port != SVI_PORT {
        return crate::ospf::l2_reach(t, End { device: d.id, port: ifc.port });
    }
    let Some(v) = ifc.vlan.and_then(packetcraft_core::VlanId::new) else { return BTreeSet::new() };
    let mut all = BTreeSet::new();
    for p in (0..d.ports.len()).filter(|p| crate::stp::carries(t, d, *p, v)) {
        all.extend(crate::ospf::l2_reach(t, End { device: d.id, port: p }));
    }
    all
}

fn reaches(t: &Topology, r: &BTreeSet<End>, b: &Device, ib: &L3If6) -> bool {
    if ib.port != SVI_PORT {
        return r.contains(&End { device: b.id, port: ib.port });
    }
    let Some(v) = ib.vlan.and_then(packetcraft_core::VlanId::new) else { return false };
    (0..b.ports.len()).any(|p| crate::stp::carries(t, b, p, v) && r.contains(&End { device: b.id, port: p }))
}

pub fn compute(t: &Topology) -> Ospf6State {
    let mut st = Ospf6State::default();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| routes6(d) && d.ospf6.is_some()).collect();
    let mut ifs: BTreeMap<DeviceId, Vec<(L3If6, u32, bool)>> = BTreeMap::new();
    for d in &routers {
        let Some(id) = router_id(d) else { continue };
        st.router_ids.insert(d.id, id);
        let list = ifaces(t, d);
        st.interfaces.insert(d.id, list.iter().map(|(i, a, p)| (i.name.clone(), *a, cost(i), *p)).collect());
        ifs.insert(d.id, list);
    }
    // Adjacencies: (from, to, from's interface, to's link-local address, cost).
    let mut adj: BTreeMap<DeviceId, Vec<(DeviceId, L3If6, Ipv6Addr, u32)>> = BTreeMap::new();
    for a in routers.iter().filter(|d| st.router_ids.contains_key(&d.id)) {
        for (ia, area_a, pa) in ifs.get(&a.id).into_iter().flatten() {
            if *pa {
                continue;
            }
            let r = reach(t, a, ia);
            for b in routers.iter().filter(|b| b.id != a.id) {
                let Some(rid) = st.router_ids.get(&b.id).copied() else { continue };
                for (ib, area_b, pb) in ifs.get(&b.id).into_iter().flatten() {
                    let same_vlan = ib.vlan == ia.vlan || ia.port == SVI_PORT || ib.port == SVI_PORT;
                    if !*pb && area_a == area_b && same_vlan && reaches(t, &r, b, ib) {
                        adj.entry(a.id).or_default().push((b.id, ia.clone(), ib.link_local, cost(ia)));
                        st.neighbors.entry(a.id).or_default().push(Neighbor6 {
                            device: b.id,
                            router_id: rid,
                            address: ib.link_local,
                            iface: ia.name.clone(),
                        });
                    }
                }
            }
        }
    }
    // What each router advertises: its interfaces' global prefixes, with cost and area.
    let prefixes: BTreeMap<DeviceId, Vec<(Ipv6Net, u32, u32)>> = ifs
        .iter()
        .map(|(id, list)| {
            let nets = list
                .iter()
                .flat_map(|(i, area, _)| i.addresses.iter().map(move |a| (a.network(), cost(i), *area)))
                .filter(|(n, _, _)| !Ipv6Net::is_link_local(n.addr))
                .collect();
            (*id, nets)
        })
        .collect();
    let originators: Vec<DeviceId> = routers
        .iter()
        .filter(|d| {
            d.ospf6.as_ref().is_some_and(|o| o.default_originate)
                && d.routes6.iter().any(|r| r.net.prefix == 0 && (r.next_hop.is_some() || r.iface.is_some()))
        })
        .map(|d| d.id)
        .collect();

    for r in routers.iter().filter(|d| st.router_ids.contains_key(&d.id)) {
        let mut best: BTreeMap<DeviceId, (u32, Option<(L3If6, Ipv6Addr)>)> = BTreeMap::new();
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
            for (v, ifc, addr, c) in adj.get(&u).into_iter().flatten() {
                let total = cu.saturating_add(*c);
                let first = hop.clone().or_else(|| Some((ifc.clone(), *addr)));
                if !done.contains(v) && best.get(v).is_none_or(|(old, _)| total < *old) {
                    best.insert(*v, (total, first));
                }
            }
        }
        let mine: BTreeSet<Ipv6Net> = up_ifaces6(t, r).iter().flat_map(|i| i.addresses.iter().map(|a| a.network())).collect();
        let my_areas: BTreeSet<u32> = ifs.get(&r.id).into_iter().flatten().map(|(_, a, _)| *a).collect();
        let mut routes: BTreeMap<Ipv6Net, Ospf6Route> = BTreeMap::new();
        for (x, (cx, hop)) in &best {
            let Some((ifc, via)) = hop else { continue };
            for (net, c, area) in prefixes.get(x).into_iter().flatten() {
                if mine.contains(net) {
                    continue;
                }
                let cand = Ospf6Route {
                    net: *net,
                    via: *via,
                    iface: ifc.name.clone(),
                    cost: cx.saturating_add(*c),
                    inter_area: !my_areas.contains(area),
                    external: false,
                };
                if routes.get(net).is_none_or(|old| cand.cost < old.cost) {
                    routes.insert(*net, cand);
                }
            }
            if originators.contains(x) && *x != r.id {
                let def = Ipv6Net::new(Ipv6Addr::UNSPECIFIED, 0);
                let cand = Ospf6Route { net: def, via: *via, iface: ifc.name.clone(), cost: *cx, inter_area: false, external: true };
                if routes.get(&def).is_none_or(|old| cand.cost < old.cost) {
                    routes.insert(def, cand);
                }
            }
        }
        st.routes.insert(r.id, routes.into_values().collect());
    }
    st
}

/// A fingerprint of everything OSPFv3 depends on.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in &t.links {
        (l.a.device, l.a.port, l.b.device, l.b.port).hash(&mut h);
    }
    for d in t.devices.iter().filter(|d| matches!(d.kind, DeviceKind::Router | DeviceKind::Switch | DeviceKind::Hub)) {
        d.id.hash(&mut h);
        format!("{:?}{:?}{}{}", d.ospf6, d.routes6, d.ipv6_routing, d.ip_routing).hash(&mut h);
        for (i, p) in d.ports.iter().enumerate() {
            (p.shutdown, port_up(t, End { device: d.id, port: i }), format!("{:?}", p.ipv6), p.routed).hash(&mut h);
            for s in &p.subifs {
                (s.vlan, s.shutdown, format!("{:?}", s.ipv6)).hash(&mut h);
            }
        }
        for s in &d.svis {
            (s.vlan, s.shutdown, format!("{:?}", s.ipv6)).hash(&mut h);
        }
        for p in &d.ports {
            format!("{:?}", p.switchport).hash(&mut h);
        }
    }
    h.finish()
}
