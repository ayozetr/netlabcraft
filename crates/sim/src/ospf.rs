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

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::{Device, DeviceKind, End, NextHop, Topology};

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
    /// This router's address on each OSPF interface.
    pub addresses: BTreeMap<(DeviceId, String), Ipv4Addr>,
    /// Interfaces running OSPF, per router: (name, network, cost, passive).
    pub interfaces: BTreeMap<DeviceId, Vec<(String, Ipv4Net, u32, bool)>>,
    /// Per router and interface: its area, priority, and the segment's DR and BDR addresses
    /// (`None` on point-to-point links, which elect none).
    pub segments: BTreeMap<(DeviceId, String), Segment>,
    /// Routers that share a link but are not neighbours, and why (timers or areas differ).
    pub mismatches: BTreeMap<DeviceId, Vec<Mismatch>>,
    /// Neighbours met across a network-type mismatch, by router: (its interface, the
    /// neighbour's address). They are adjacent, but SPF routes nothing over the link.
    pub type_mismatch: BTreeMap<DeviceId, Vec<(String, Ipv4Addr)>>,
    /// Networks each router injects as external routes (`redistribute static`,
    /// `default-information originate`).
    pub externals: BTreeMap<DeviceId, Vec<Ipv4Net>>,
}

/// Why a router on the same link is not a neighbour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// This router's interface.
    pub iface: String,
    /// The other router's address on the link.
    pub address: Ipv4Addr,
    pub why: String,
}

/// An interface's OSPF settings (defaults when none were typed).
pub fn if_conf(d: &Device, name: &str) -> netlabcraft_model::OspfIf {
    d.ospf_if.get(name).copied().unwrap_or_default()
}

/// An interface's hello and dead intervals, in seconds (10 and 40 unless configured; the dead
/// interval follows the hello interval, four times it, when only that is set).
pub fn timers(d: &Device, name: &str) -> (u16, u16) {
    let c = if_conf(d, name);
    let hello = c.hello.unwrap_or(10);
    (hello, c.dead.unwrap_or(hello.saturating_mul(4)))
}

/// Whether an interface is point to point for OSPF: serial lines, tunnels, loopbacks, and any
/// interface set with `ip ospf network point-to-point`. No DR or BDR is elected there.
pub fn point_to_point(d: &Device, ifc: &L3If) -> bool {
    ifc.name.starts_with("Serial") || ifc.name.starts_with("Tunnel") || crate::loopback::is_loopback(ifc.port) || if_conf(d, &ifc.name).p2p
}

/// Whether a name typed in a `passive-interface` line names this interface.
fn names(typed: &str, name: &str) -> bool {
    if let (Some(a), Some(b)) = (netlabcraft_model::routing::loopback_number(typed), netlabcraft_model::routing::loopback_number(name)) {
        return a == b;
    }
    crate::vpn::same_name(typed, name)
}

/// Whether an interface is passive: listed, or every interface but the listed exceptions with
/// `passive-interface default`.
pub fn is_passive(o: &netlabcraft_model::Ospf, name: &str) -> bool {
    if o.passive_default { !o.not_passive.iter().any(|p| names(p, name)) } else { o.passive.iter().any(|p| names(p, name)) }
}

/// This router's address on an interface, from the computed state.
pub(crate) fn iface_addr(st: &OspfState, d: DeviceId, name: &str) -> Option<Ipv4Addr> {
    st.addresses.get(&(d, name.to_string())).copied()
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
    } else if ifc.name.starts_with("Loopback") {
        // A loopback costs 1 whatever the reference bandwidth.
        return 1;
    } else if ifc.name.starts_with("Tunnel") {
        // A tunnel's nominal bandwidth: 100 kbit/s.
        100
    } else {
        10_000
    };
    u32::try_from((u64::from(reference) * 1_000 / kbps).max(1)).unwrap_or(u32::MAX)
}

/// Why a received hello can't make its sender a neighbour on `ifc`, if it can't: the timers
/// or the area differ from this end's.
pub fn hello_problem(d: &Device, ifc: &L3If, h: &netlabcraft_wire::control::OspfHello) -> Option<String> {
    let (hello, dead) = timers(d, &ifc.name);
    if (h.hello_secs, h.dead_secs) != (hello, u32::from(dead)) {
        return Some(format!("hello/dead intervals {}/{} s, here {hello}/{dead} s", h.hello_secs, h.dead_secs));
    }
    let area = area_of(d, ifc);
    (h.area != area).then(|| format!("area {} in the hello, here area {area}", h.area))
}

/// The network an interface puts in its router's LSA: a loopback stands for one host (a /32
/// stub), every other interface for its subnet.
pub fn advertised(d: &Device, ifc: &L3If) -> Ipv4Net {
    if crate::loopback::is_loopback(ifc.port) && !if_conf(d, &ifc.name).p2p {
        return Ipv4Net::new(ifc.net.addr, 32);
    }
    ifc.net.network()
}

/// The interfaces of a router OSPF runs on.
fn ospf_ifaces(t: &Topology, d: &Device) -> Vec<(L3If, bool)> {
    let Some(o) = &d.ospf else { return Vec::new() };
    up_ifaces(t, d)
        .into_iter()
        .filter(|i| if_conf(d, &i.name).area.is_some_and(|(p, _)| p == o.process) || o.networks.iter().any(|n| n.matches(i.net.addr)))
        .map(|i| {
            let passive = is_passive(o, &i.name);
            (i, passive)
        })
        .collect()
}

/// The router id: configured, else the highest interface address.
pub fn router_id(d: &Device) -> Option<Ipv4Addr> {
    let o = d.ospf.as_ref()?;
    o.router_id.or_else(|| crate::loopback::default_router_id(d))
}

/// The area of the network statement an interface address falls under.
fn area_of(d: &Device, ifc: &L3If) -> u32 {
    // `ip ospf N area A` on the interface wins over the network statements.
    if let (Some(o), Some((p, a))) = (&d.ospf, if_conf(d, &ifc.name).area)
        && p == o.process
    {
        return a;
    }
    let a = ifc.net.addr;
    d.ospf.as_ref().and_then(|o| o.networks.iter().find(|n| n.matches(a))).map_or(0, |n| n.area)
}

/// Whether an interface's area is a stub area on this router (both ends must agree, or they
/// don't become neighbours: the hellos' E bit differs).
fn stub_here(d: &Device, ifc: &L3If) -> bool {
    d.ospf.as_ref().is_some_and(|o| o.area(area_of(d, ifc)).stub)
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
    let Some(v) = ifc.vlan.and_then(netlabcraft_core::VlanId::new) else { return BTreeSet::new() };
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
    let Some(v) = ib.vlan.and_then(netlabcraft_core::VlanId::new) else { return false };
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
        st.interfaces.insert(d.id, list.iter().map(|(i, p)| (i.name.clone(), advertised(d, i), cost(d, i), *p)).collect());
        for (i, _) in &list {
            st.addresses.insert((d.id, i.name.clone()), i.net.addr);
        }
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
                    if *pb || !same || !reaches(t, &reach, b, ib) {
                        continue;
                    }
                    // Hellos must agree on the timers, the area and the stub flag (RFC 2328 §10.5).
                    let (ha, da) = timers(a, &ia.name);
                    let (hb, db) = timers(b, &ib.name);
                    let why = if (ha, da) != (hb, db) {
                        Some(format!("hello/dead intervals differ ({ha}/{da} s here, {hb}/{db} s there)"))
                    } else if area_of(a, ia) != area_of(b, ib) {
                        Some(format!("areas differ ({} here, {} there)", area_of(a, ia), area_of(b, ib)))
                    } else if stub_here(a, ia) != stub_here(b, ib) {
                        Some("one end is a stub area and the other is not".to_string())
                    } else {
                        None
                    };
                    if let Some(why) = why {
                        st.mismatches.entry(a.id).or_default().push(Mismatch { iface: ia.name.clone(), address: ib.net.addr, why });
                        continue;
                    }
                    // A network-type mismatch (point-to-point at one end, broadcast at the
                    // other) passes the hello checks (RFC 2328 §10.5), so the neighbours meet;
                    // but their router-LSAs describe the link differently (a point-to-point
                    // link, a transit network), the SPF two-way check (§16.1 (2b)) fails and
                    // no route crosses it.
                    if point_to_point(a, ia) != point_to_point(b, ib) {
                        st.type_mismatch.entry(a.id).or_default().push((ia.name.clone(), ib.net.addr));
                    }
                    {
                        if point_to_point(a, ia) == point_to_point(b, ib) {
                            adj.entry(a.id).or_default().push((b.id, ia.clone(), ib.net.addr, cost(a, ia)));
                        }
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
            let area = area_of(d, ifc);
            let p2p = point_to_point(d, ifc);
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
        .map(|d| (d.id, ifs.get(&d.id).into_iter().flatten().map(|(i, _)| (advertised(d, i), cost(d, i), area_of(d, i))).collect()))
        .collect();
    let originators: Vec<DeviceId> = routers
        .iter()
        .filter(|d| {
            d.ospf.as_ref().is_some_and(|o| o.default_originate)
                && d.routes.iter().any(|r| r.net.prefix == 0 && matches!(r.via, NextHop::Address(_) | NextHop::Interface(_)))
        })
        .map(|d| d.id)
        .collect();
    // Static routes redistributed into OSPF, per router (the ones in its routing table).
    let redistributed: BTreeMap<DeviceId, Vec<Ipv4Net>> = routers
        .iter()
        .filter(|d| d.ospf.as_ref().is_some_and(|o| o.redistribute_static))
        .map(|d| (d.id, crate::installed_statics(t, d).into_iter().filter(|n| n.prefix > 0).collect()))
        .collect();
    for (d, nets) in &redistributed {
        st.externals.entry(*d).or_default().extend(nets.iter().copied());
    }
    for d in &originators {
        st.externals.entry(*d).or_default().push(Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0));
    }
    let areas_of: BTreeMap<DeviceId, BTreeSet<u32>> =
        routers.iter().map(|d| (d.id, ifs.get(&d.id).into_iter().flatten().map(|(i, _)| area_of(d, i)).collect())).collect();
    // Border routers of an area: in it and in another.
    let abrs = |area: u32| -> Vec<DeviceId> { areas_of.iter().filter(|(_, a)| a.len() > 1 && a.contains(&area)).map(|(d, _)| *d).collect() };
    let conf = |d: DeviceId, area: u32| t.device(d).and_then(|x| x.ospf.as_ref()).map(|o| o.area(area)).unwrap_or_default();
    for r in &routers {
        // Dijkstra: (cost, first hops: interface and address) per router; equal-cost paths keep
        // every first hop (up to `maximum-paths`).
        let max_paths = usize::from(r.ospf.as_ref().and_then(|o| o.maximum_paths).unwrap_or(4).max(1));
        let mut best: BTreeMap<DeviceId, (u32, Vec<(L3If, Ipv4Addr)>)> = BTreeMap::new();
        best.insert(r.id, (0, Vec::new()));
        let mut done: BTreeSet<DeviceId> = BTreeSet::new();
        loop {
            let next = best
                .iter()
                .filter(|(d, _)| !done.contains(*d))
                .min_by_key(|(d, (c, _))| (*c, st.router_ids.get(*d).copied()))
                .map(|(d, v)| (*d, v.clone()));
            let Some((u, (cu, hops))) = next else { break };
            done.insert(u);
            for (v, ifc, addr, cost) in adj.get(&u).into_iter().flatten() {
                if done.contains(v) {
                    continue;
                }
                let c = cu.saturating_add(*cost);
                let first = if hops.is_empty() { vec![(ifc.clone(), *addr)] } else { hops.clone() };
                match best.get_mut(v) {
                    Some((old, oh)) if c == *old => {
                        for f in first {
                            if !oh.iter().any(|x| x.0.name == f.0.name && x.1 == f.1) {
                                oh.push(f);
                            }
                        }
                        oh.sort_by_key(|x| x.1);
                        oh.truncate(max_paths);
                    }
                    Some((old, _)) if c > *old => {}
                    _ => {
                        let mut first = first;
                        first.sort_by_key(|x| x.1);
                        first.truncate(max_paths);
                        best.insert(*v, (c, first));
                    }
                }
            }
        }
        let mine: Vec<Ipv4Net> = up_ifaces(t, r).iter().map(|i| i.net.network()).collect();
        let my_areas: BTreeSet<u32> = ifs.get(&r.id).into_iter().flatten().map(|(i, _)| area_of(r, i)).collect();
        // A router wholly inside a stub area hears no external routes; with no-summary on its
        // border router, no routes from other areas either.
        let stub_area = my_areas.first().copied().filter(|a| my_areas.len() == 1 && conf(r.id, *a).stub);
        let totally = stub_area.is_some_and(|a| abrs(a).iter().any(|b| conf(*b, a).no_summary));
        let mut routes: BTreeMap<Ipv4Net, OspfRoute> = BTreeMap::new();
        // Other paths of the same cost to a destination, beside the one in `routes`.
        let mut more: BTreeMap<Ipv4Net, Vec<OspfRoute>> = BTreeMap::new();
        let mut add = |routes: &mut BTreeMap<Ipv4Net, OspfRoute>, cand: OspfRoute| match routes.get(&cand.net) {
            Some(old) if old.cost == cand.cost && old.external == cand.external => {
                let list = more.entry(cand.net).or_default();
                if old.via != cand.via && !list.iter().any(|x| x.via == cand.via) {
                    list.push(cand);
                }
            }
            Some(old) if (old.cost, old.via) <= (cand.cost, cand.via) => {}
            _ => {
                more.remove(&cand.net);
                routes.insert(cand.net, cand);
            }
        };
        for (x, (cx, hops)) in &best {
            for (ifc, via) in hops {
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
                    add(&mut routes, cand);
                }
                for net in redistributed.get(x).into_iter().flatten().filter(|_| *x != r.id && stub_area.is_none()) {
                    if mine.contains(net) {
                        continue;
                    }
                    // Type-2 externals keep the metric they were given (20) whatever the path.
                    let cand = OspfRoute { net: *net, via: *via, iface: ifc.name.clone(), cost: 20, external: true, inter_area: false };
                    add(&mut routes, cand);
                }
                if originators.contains(x) && *x != r.id && stub_area.is_none() {
                    let def = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0);
                    let cand = OspfRoute { net: def, via: *via, iface: ifc.name.clone(), cost: 1, external: true, inter_area: false };
                    if routes.get(&def).is_none_or(|old| (*cx, cand.via) < (old.cost.saturating_add(*cx), old.via)) {
                        routes.insert(def, cand);
                    }
                }
            }
        }
        // The stub area's border routers send a default route in its place.
        if let Some(a) = stub_area {
            let def = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0);
            for b in abrs(a) {
                if let Some((cx, hops)) = best.get(&b)
                    && let Some((ifc, via)) = hops.first()
                {
                    let cand =
                        OspfRoute { net: def, via: *via, iface: ifc.name.clone(), cost: cx.saturating_add(1), external: false, inter_area: true };
                    if routes.get(&def).is_none_or(|old| (cand.cost, cand.via) < (old.cost, old.via)) {
                        routes.insert(def, cand);
                    }
                }
            }
        }
        let mut list = Vec::new();
        for (net, first) in routes {
            list.push(first);
            let mut extra = more.remove(&net).unwrap_or_default();
            extra.sort_by_key(|x| x.via);
            list.extend(extra.into_iter().take(max_paths.saturating_sub(1)));
        }
        st.routes.insert(r.id, list);
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
            format!("{:?}{:?}{:?}{:?}{:?}{:?}{:?}{}", d.ospf, d.rip, d.eigrp, d.routes, d.access.users, d.ospf_if, d.eigrp_if, d.ip_routing)
                .hash(&mut h);
            d.eigrp_summary.hash(&mut h);
            d.tunnels.hash(&mut h);
            format!("{:?}", d.loopbacks).hash(&mut h);
            for s in &d.svis {
                (s.vlan, s.ip, s.shutdown).hash(&mut h);
            }
        }
    }
    h.finish()
}
