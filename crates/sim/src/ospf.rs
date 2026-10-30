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
    /// The network is in an area this router has no interface in (an inter-area route).
    pub inter_area: bool,
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
    /// Per router and interface: its area, priority, and the segment's DR and BDR addresses
    /// (`None` on point-to-point links, which elect none).
    pub segments: BTreeMap<(DeviceId, String), Segment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub area: u32,
    pub priority: u8,
    pub dr: Option<Ipv4Addr>,
    pub bdr: Option<Ipv4Addr>,
}

/// An interface's OSPF cost: configured, else the reference bandwidth over its speed.
pub fn cost(d: &Device, ifc: &L3If) -> u32 {
    if let Some(c) = d.ospf_if.get(&ifc.name).and_then(|o| o.cost) {
        return c.max(1);
    }
    let reference = d.ospf.as_ref().and_then(|o| o.reference_bw).unwrap_or(100);
    let kbps: u64 = if ifc.name.starts_with("GigabitEthernet") {
        1_000_000
    } else if ifc.name.starts_with("FastEthernet") || ifc.name.starts_with("Vlan") {
        100_000
    } else if ifc.name.starts_with("Serial") {
        1_544
    } else if ifc.name.starts_with("Tunnel") {
        // A tunnel's nominal bandwidth: 100 kbit/s.
        100
    } else {
        10_000
    };
    u32::try_from((u64::from(reference) * 1_000 / kbps).max(1)).unwrap_or(u32::MAX)
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

/// The area of the network statement an interface address falls under.
fn area_of(d: &Device, a: Ipv4Addr) -> u32 {
    d.ospf.as_ref().and_then(|o| o.networks.iter().find(|n| n.matches(a))).map_or(0, |n| n.area)
}

/// Whether an interface's area is a stub area on this router (both ends must agree, or they
/// don't become neighbours: the hellos' E bit differs).
fn stub_here(d: &Device, ifc: &L3If) -> bool {
    d.ospf.as_ref().is_some_and(|o| o.area(area_of(d, ifc.net.addr)).stub)
}

/// Ports reachable at layer 2 from an OSPF interface: from its port, or for a switch's VLAN
/// interface from every port of that VLAN.
pub(crate) fn iface_reach(t: &Topology, d: &Device, ifc: &L3If) -> BTreeSet<End> {
    if crate::vpn::is_tunnel(ifc.port) {
        // A tunnel reaches the far end of the tunnel, point to point.
        return crate::vpn::tunnel_peers(t, d, ifc);
    }
    if ifc.port != crate::SVI_PORT {
        return l2_reach(t, End { device: d.id, port: ifc.port });
    }
    let Some(v) = ifc.vlan.and_then(packetcraft_core::VlanId::new) else { return BTreeSet::new() };
    let mut all = BTreeSet::new();
    for p in (0..d.ports.len()).filter(|p| crate::stp::carries(t, d, *p, v)) {
        all.extend(l2_reach(t, End { device: d.id, port: p }));
    }
    all
}

/// Whether a reach set gets to another router's interface (a switch's VLAN interface counts
/// when any port of its VLAN is reached).
pub(crate) fn reaches(t: &Topology, reach: &BTreeSet<End>, b: &Device, ib: &L3If) -> bool {
    if ib.port != crate::SVI_PORT {
        return reach.contains(&End { device: b.id, port: ib.port });
    }
    let Some(v) = ib.vlan.and_then(packetcraft_core::VlanId::new) else { return false };
    (0..b.ports.len()).any(|p| crate::stp::carries(t, b, p, v) && reach.contains(&End { device: b.id, port: p }))
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
    let routers: Vec<&Device> = t.devices.iter().filter(|d| d.routes_ip() && d.ospf.is_some()).collect();
    let mut ifs: BTreeMap<DeviceId, Vec<(L3If, bool)>> = BTreeMap::new();
    for d in &routers {
        if let Some(id) = router_id(d) {
            st.router_ids.insert(d.id, id);
        }
        let list = ospf_ifaces(t, d);
        st.interfaces.insert(d.id, list.iter().map(|(i, p)| (i.name.clone(), i.net.network(), cost(d, i), *p)).collect());
        ifs.insert(d.id, list);
    }
    // Adjacencies: (from, to, from's interface, to's address, cost).
    let mut adj: BTreeMap<DeviceId, Vec<(DeviceId, L3If, Ipv4Addr, u32)>> = BTreeMap::new();
    for a in &routers {
        for (ia, pa) in ifs.get(&a.id).into_iter().flatten() {
            if *pa {
                continue;
            }
            let reach = iface_reach(t, a, ia);
            for b in &routers {
                if b.id == a.id {
                    continue;
                }
                for (ib, pb) in ifs.get(&b.id).into_iter().flatten() {
                    let same =
                        ib.net.network() == ia.net.network() && (ib.vlan == ia.vlan || ia.port == crate::SVI_PORT || ib.port == crate::SVI_PORT);
                    if !*pb && same && stub_here(a, ia) == stub_here(b, ib) && reaches(t, &reach, b, ib) {
                        adj.entry(a.id).or_default().push((b.id, ia.clone(), ib.net.addr, cost(a, ia)));
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
    // DR and BDR per segment: highest priority, then router id; priority 0 never; none on
    // point-to-point (serial) links.
    for d in &routers {
        let Some(rid) = st.router_ids.get(&d.id).copied() else { continue };
        for (ifc, _) in ifs.get(&d.id).into_iter().flatten() {
            let prio = |dev: &Device, name: &str| dev.ospf_if.get(name).and_then(|o| o.priority).unwrap_or(1);
            let area = area_of(d, ifc.net.addr);
            let p2p = ifc.name.starts_with("Serial") || ifc.name.starts_with("Tunnel");
            let mut seg: Vec<(u8, Ipv4Addr, Ipv4Addr)> = vec![(prio(d, &ifc.name), rid, ifc.net.addr)];
            for n in st.neighbors.get(&d.id).into_iter().flatten().filter(|n| n.iface == ifc.name) {
                let their_if =
                    t.device(n.device).and_then(|nd| crate::l3_ifaces(nd).into_iter().find(|i| i.net.addr == n.address).map(|i| prio(nd, &i.name)));
                seg.push((their_if.unwrap_or(1), n.router_id, n.address));
            }
            seg.retain(|x| x.0 > 0);
            seg.sort_by_key(|x| std::cmp::Reverse((x.0, x.1)));
            let (dr, bdr) = if p2p { (None, None) } else { (seg.first().map(|x| x.2), seg.get(1).map(|x| x.2)) };
            st.segments.insert((d.id, ifc.name.clone()), Segment { area, priority: prio(d, &ifc.name), dr, bdr });
        }
    }
    // Networks each router advertises (cost of its own interface added), with their area.
    let stubs: BTreeMap<DeviceId, Vec<(Ipv4Net, u32, u32)>> = routers
        .iter()
        .map(|d| (d.id, ifs.get(&d.id).into_iter().flatten().map(|(i, _)| (i.net.network(), cost(d, i), area_of(d, i.net.addr))).collect()))
        .collect();
    let originators: Vec<DeviceId> = routers
        .iter()
        .filter(|d| {
            d.ospf.as_ref().is_some_and(|o| o.default_originate)
                && d.routes.iter().any(|r| r.net.prefix == 0 && matches!(r.via, NextHop::Address(_) | NextHop::Interface(_)))
        })
        .map(|d| d.id)
        .collect();
    let areas_of: BTreeMap<DeviceId, BTreeSet<u32>> =
        routers.iter().map(|d| (d.id, ifs.get(&d.id).into_iter().flatten().map(|(i, _)| area_of(d, i.net.addr)).collect())).collect();
    // Border routers of an area: in it and in another.
    let abrs = |area: u32| -> Vec<DeviceId> { areas_of.iter().filter(|(_, a)| a.len() > 1 && a.contains(&area)).map(|(d, _)| *d).collect() };
    let conf = |d: DeviceId, area: u32| t.device(d).and_then(|x| x.ospf.as_ref()).map(|o| o.area(area)).unwrap_or_default();
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
        let my_areas: BTreeSet<u32> = ifs.get(&r.id).into_iter().flatten().map(|(i, _)| area_of(r, i.net.addr)).collect();
        // A router wholly inside a stub area hears no external routes; with no-summary on its
        // border router, no routes from other areas either.
        let stub_area = my_areas.first().copied().filter(|a| my_areas.len() == 1 && conf(r.id, *a).stub);
        let totally = stub_area.is_some_and(|a| abrs(a).iter().any(|b| conf(*b, a).no_summary));
        let mut routes: BTreeMap<Ipv4Net, OspfRoute> = BTreeMap::new();
        for (x, (cx, hop)) in &best {
            let Some((ifc, via)) = hop else { continue };
            for (net, c, area) in stubs.get(x).into_iter().flatten() {
                if mine.contains(net) {
                    continue;
                }
                let inter_area = !my_areas.contains(area);
                if inter_area && totally {
                    continue;
                }
                // A border router of the network's area may summarise it.
                let summary = if inter_area {
                    abrs(*area).iter().flat_map(|b| conf(*b, *area).ranges).find(|g| g.prefix <= net.prefix && g.contains(net.addr))
                } else {
                    None
                };
                let net = &summary.unwrap_or(*net);
                if mine.contains(net) {
                    continue;
                }
                let cand = OspfRoute { net: *net, via: *via, iface: ifc.name.clone(), cost: cx.saturating_add(*c), external: false, inter_area };
                if routes.get(net).is_none_or(|old| (cand.cost, cand.via) < (old.cost, old.via)) {
                    routes.insert(*net, cand);
                }
            }
            if originators.contains(x) && *x != r.id && stub_area.is_none() {
                let def = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0);
                let cand = OspfRoute { net: def, via: *via, iface: ifc.name.clone(), cost: 1, external: true, inter_area: false };
                if routes.get(&def).is_none_or(|old| (*cx, cand.via) < (old.cost.saturating_add(*cx), old.via)) {
                    routes.insert(def, cand);
                }
            }
        }
        // The stub area's border routers send a default route in its place.
        if let Some(a) = stub_area {
            let def = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0);
            for b in abrs(a) {
                if let Some((cx, Some((ifc, via)))) = best.get(&b) {
                    let cand =
                        OspfRoute { net: def, via: *via, iface: ifc.name.clone(), cost: cx.saturating_add(1), external: false, inter_area: true };
                    if routes.get(&def).is_none_or(|old| (cand.cost, cand.via) < (old.cost, old.via)) {
                        routes.insert(def, cand);
                    }
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
            // Serial settings and users decide whether a PPP link comes up.
            (p.shutdown, p.ip, &p.serial, p.bandwidth, p.delay).hash(&mut h);
            for s in &p.subifs {
                (s.ip, s.vlan, s.shutdown).hash(&mut h);
            }
        }
        if matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) {
            format!("{:?}{:?}{:?}{:?}{:?}{:?}{}", d.ospf, d.rip, d.eigrp, d.routes, d.access.users, d.ospf_if, d.ip_routing).hash(&mut h);
            d.tunnels.hash(&mut h);
            for s in &d.svis {
                (s.vlan, s.ip, s.shutdown).hash(&mut h);
            }
        }
    }
    h.finish()
}
