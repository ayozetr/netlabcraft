//! OSPFv3 (RFC 5340): OSPF for IPv6, computed from the topology like OSPFv2.
//!
//! Interfaces join a process with `ipv6 ospf N area A`; neighbours meet over link-local
//! addresses on a shared link (their global prefixes don't have to match), and routes lead to
//! the neighbour's link-local address. Hellos go on the wire every 10 s (`control`).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use netlabcraft_core::{DeviceId, Ipv6Net};
use netlabcraft_model::{Device, DeviceKind, End, Ospf6Auth, Ospf6AuthAlg, Topology};
use netlabcraft_wire::ah::Ah;
use netlabcraft_wire::ipv6::Ipv6;
use netlabcraft_wire::ospf6::{ALL_SPF_ROUTERS6, Hello6, PROTO_OSPF};
use netlabcraft_wire::suite::Integ;

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
    /// Per router: the routers it shares a link and an area with but did not become neighbours
    /// with over authentication: (our interface, their router ID, why).
    pub refused: BTreeMap<DeviceId, Vec<(String, Ipv4Addr, String)>>,
}

/// The protection an interface puts on its OSPFv3 packets: its own setting, else its area's.
/// `None` for no protection (including an explicit `null`).
pub fn auth_of<'a>(d: &'a Device, ifc: &L3If6, area: u32) -> Option<&'a Ospf6Auth> {
    let own = conf6(d, ifc).and_then(|c| c.ospf_auth.as_ref());
    own.or_else(|| d.ospf6.as_ref().and_then(|o| o.area_auth.get(&area))).filter(|a| !matches!(a, Ospf6Auth::Null))
}

/// The integrity algorithm and key bytes of an IPsec setting. The key is hexadecimal; a lab
/// file that holds anything else gets the text's own bytes.
pub fn ah_suite(alg: Ospf6AuthAlg, key: &str) -> (Integ, Vec<u8>) {
    let integ = match alg {
        Ospf6AuthAlg::Md5 => Integ::HmacMd5_96,
        Ospf6AuthAlg::Sha1 => Integ::HmacSha1_96,
    };
    let digits: Vec<u8> = key.bytes().filter_map(|b| char::from(b).to_digit(16).and_then(|v| u8::try_from(v).ok())).collect();
    let bytes = if digits.len() == key.len() && digits.len().is_multiple_of(2) {
        digits.chunks(2).map(|p| (p.first().copied().unwrap_or(0) << 4) | p.get(1).copied().unwrap_or(0)).collect()
    } else {
        key.as_bytes().to_vec()
    };
    (integ, bytes)
}

/// The packet an OSPFv3 router sends: the protocol-89 packet from the link-local address to
/// `ff02::5`, wrapped in an authentication header when the interface has one (`seq` is the
/// header's sequence number).
pub fn wrap(src: Ipv6Addr, payload: Vec<u8>, auth: Option<&Ospf6Auth>, seq: u32) -> Ipv6 {
    let plain = Ipv6 { hop_limit: 1, next_header: PROTO_OSPF, src, dst: ALL_SPF_ROUTERS6, payload };
    match auth {
        Some(Ospf6Auth::Ipsec { spi, alg, key }) => {
            let (integ, bytes) = ah_suite(*alg, key);
            Ah::protect_transport6(integ, &bytes, &plain, *spi, seq)
        }
        _ => plain,
    }
}

/// Why a router whose interface is protected by `rx` rejects a packet that `packet` is, if it
/// does: an authentication header it did not ask for, none when it wants one, another SPI, or
/// a check value that does not verify under its key.
pub fn receive_problem(rx: Option<&Ospf6Auth>, packet: &Ipv6) -> Option<String> {
    let has_ah = packet.next_header == netlabcraft_wire::ah::PROTO_AH;
    match (rx, has_ah) {
        (None, false) => None,
        (None, true) => Some("the packet carries an AH header but this interface has no OSPFv3 authentication".into()),
        (Some(Ospf6Auth::Ipsec { .. }), false) => Some("the packet has no AH header but this interface requires one".into()),
        (Some(Ospf6Auth::Ipsec { spi, alg, key }), true) => {
            let (integ, bytes) = ah_suite(*alg, key);
            match Ah::open_transport6(packet, integ, &bytes, *spi) {
                Ok(_) => None,
                Err(netlabcraft_wire::WireError::Checksum(_)) => Some("the AH integrity check value does not verify (the keys differ)".into()),
                Err(_) => Some(format!("the AH header names another SPI or is malformed (this interface expects SPI {spi})")),
            }
        }
        (Some(Ospf6Auth::Null), _) => None,
    }
}

/// Why two interfaces on one link can't become OSPFv3 neighbours over authentication: a hello
/// from each, protected the way its sender protects it, is checked the way the other end checks it.
fn auth_mismatch(a: &Device, ia: &L3If6, area_a: u32, b: &Device, ib: &L3If6, area_b: u32) -> Option<String> {
    let (ea, eb) = (auth_of(a, ia, area_a), auth_of(b, ib, area_b));
    if ea.is_none() && eb.is_none() {
        return None;
    }
    let probe = |from: &L3If6| {
        Hello6 {
            router_id: Ipv4Addr::UNSPECIFIED,
            area: 0,
            interface_id: 1,
            priority: 1,
            dr: Ipv4Addr::UNSPECIFIED,
            bdr: Ipv4Addr::UNSPECIFIED,
            neighbors: Vec::new(),
        }
        .encode(from.link_local, ALL_SPF_ROUTERS6)
    };
    let there = wrap(ia.link_local, probe(ia), ea, 1);
    if let Some(why) = receive_problem(eb, &there) {
        return Some(format!("authentication fails: the neighbour rejects this router's packets ({why})"));
    }
    let here = wrap(ib.link_local, probe(ib), eb, 1);
    receive_problem(ea, &here).map(|why| format!("authentication fails: this router rejects the neighbour's packets ({why})"))
}

/// The router ID: configured, else the highest IPv4 interface address. OSPFv3 doesn't start
/// without one.
pub fn router_id(d: &Device) -> Option<Ipv4Addr> {
    let o = d.ospf6.as_ref()?;
    o.router_id.or_else(|| crate::loopback::default_router_id(d))
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
            let passive = o.passive.iter().any(|p| netlabcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == i.name));
            (process == o.process).then_some((i, area, passive))
        })
        .collect()
}

/// Ports reachable at layer 2 from an interface.
pub(crate) fn reach(t: &Topology, d: &Device, ifc: &L3If6) -> BTreeSet<End> {
    if ifc.port != SVI_PORT {
        return crate::ospf::l2_reach(t, End { device: d.id, port: ifc.port });
    }
    let Some(v) = ifc.vlan.and_then(netlabcraft_core::VlanId::new) else { return BTreeSet::new() };
    let mut all = BTreeSet::new();
    for p in (0..d.ports.len()).filter(|p| crate::stp::carries(t, d, *p, v)) {
        all.extend(crate::ospf::l2_reach(t, End { device: d.id, port: p }));
    }
    all
}

pub(crate) fn reaches(t: &Topology, r: &BTreeSet<End>, b: &Device, ib: &L3If6) -> bool {
    if ib.port != SVI_PORT {
        return r.contains(&End { device: b.id, port: ib.port });
    }
    let Some(v) = ib.vlan.and_then(netlabcraft_core::VlanId::new) else { return false };
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
                        if let Some(why) = auth_mismatch(a, ia, *area_a, b, ib, *area_b) {
                            st.refused.entry(a.id).or_default().push((ia.name.clone(), rid, why));
                            continue;
                        }
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
        format!("{:?}", d.loopbacks).hash(&mut h);
        for p in &d.ports {
            format!("{:?}", p.switchport).hash(&mut h);
        }
    }
    h.finish()
}
