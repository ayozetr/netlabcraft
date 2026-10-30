//! What OSPF's link-state database holds once the protocol has converged (RFC 2328 §12.4),
//! derived from the computed [`OspfState`]: a router-LSA per router and area, a network-LSA per
//! segment with a designated router, summary-LSAs from the area border routers, and
//! AS-external-LSAs for redistributed and default routes. Also the memory of past DR elections,
//! which makes the election non-preemptive as the standard has it, and `clear ip ospf process`.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::Topology;

use crate::ospf::{OspfState, Segment};

/// The kinds of LSA `show ip ospf database` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LsaKind {
    Router = 1,
    Network = 2,
    Summary = 3,
    External = 5,
}

/// One LSA as the database lists it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lsa {
    pub kind: LsaKind,
    /// The area it floods in (`None` for AS-external LSAs, which flood everywhere).
    pub area: Option<u32>,
    /// Link state ID: the router ID (router-LSA), the DR's address (network-LSA), the network
    /// (summary and external LSAs).
    pub id: Ipv4Addr,
    pub adv: Ipv4Addr,
    /// Router-LSA: its links. Others: the network's prefix length.
    pub count: u32,
}

/// The database a router holds: the LSAs of every area it has an interface in, and the
/// external ones.
pub fn database(t: &Topology, st: &OspfState, me: DeviceId) -> Vec<Lsa> {
    let my_areas: BTreeSet<u32> = st.segments.iter().filter(|((d, _), _)| *d == me).map(|(_, s)| s.area).collect();
    let rid = |d: DeviceId| st.router_ids.get(&d).copied();
    let heard = |d: DeviceId, iface: &str| st.neighbors.get(&d).into_iter().flatten().filter(|n| n.iface == iface).count();
    let mut out: BTreeSet<Lsa> = BTreeSet::new();
    // Areas each router is in, and the networks of each area.
    let mut areas_of: BTreeMap<DeviceId, BTreeSet<u32>> = BTreeMap::new();
    let mut nets_in: BTreeMap<u32, BTreeSet<Ipv4Net>> = BTreeMap::new();
    for ((d, iface), seg) in &st.segments {
        areas_of.entry(*d).or_default().insert(seg.area);
        if let Some((_, net, _, _)) = st.interfaces.get(d).into_iter().flatten().find(|x| x.0 == *iface) {
            nets_in.entry(seg.area).or_default().insert(*net);
        }
    }
    for area in &my_areas {
        for (d, areas) in areas_of.iter().filter(|(_, a)| a.contains(area)) {
            let Some(r) = rid(*d) else { continue };
            // Router-LSA: point-to-point links count twice (the neighbour and the subnet), a
            // segment with neighbours once (its transit network), anything else once (a stub).
            let mut links = 0u32;
            for (name, ..) in st.interfaces.get(d).into_iter().flatten() {
                let Some(seg) = st.segments.get(&(*d, name.clone())).filter(|s| s.area == *area) else { continue };
                let n = u32::try_from(heard(*d, name)).unwrap_or(u32::MAX);
                links = links.saturating_add(if seg.dr.is_none() { n.saturating_add(1) } else { 1 });
            }
            out.insert(Lsa { kind: LsaKind::Router, area: Some(*area), id: r, adv: r, count: links });
            // Network-LSAs: from the DR of each segment that has one and neighbours on it.
            for ((dev, name), seg) in st.segments.iter().filter(|((dev, _), s)| dev == d && s.area == *area) {
                if let (Some(dr), true) = (seg.dr, heard(*dev, name) > 0) {
                    let owner = owner_of(t, st, dr).and_then(rid);
                    if let Some(adv) = owner {
                        let prefix = st.interfaces.get(dev).into_iter().flatten().find(|x| x.0 == *name).map_or(0, |x| x.1.prefix);
                        out.insert(Lsa { kind: LsaKind::Network, area: Some(*area), id: dr, adv, count: u32::from(prefix) });
                    }
                }
            }
            // Summary-LSAs: an area border router tells this area about the others' networks.
            if areas.len() > 1 {
                for (other, nets) in nets_in.iter().filter(|(a, _)| *a != area) {
                    let _ = other;
                    for n in nets {
                        if nets_in.get(area).is_some_and(|mine| mine.contains(n)) {
                            continue;
                        }
                        out.insert(Lsa { kind: LsaKind::Summary, area: Some(*area), id: n.addr, adv: r, count: u32::from(n.prefix) });
                    }
                }
            }
        }
    }
    for (d, nets) in &st.externals {
        let Some(r) = rid(*d) else { continue };
        for n in nets {
            out.insert(Lsa { kind: LsaKind::External, area: None, id: n.addr, adv: r, count: u32::from(n.prefix) });
        }
    }
    out.into_iter().collect()
}

/// The router that owns an interface address.
fn owner_of(t: &Topology, st: &OspfState, a: Ipv4Addr) -> Option<DeviceId> {
    st.router_ids.keys().copied().find(|d| t.device(*d).is_some_and(|x| crate::l3_ifaces(x).iter().any(|i| i.net.addr == a)))
}

/// Applies the elections remembered from `old` to a fresh state: a DR (and BDR) chosen while
/// routers were neighbours keeps its role while it stays on the segment, whatever routers with
/// a higher priority or router ID join later (RFC 2328 §9.4). When the DR leaves, the BDR takes
/// over.
pub(crate) fn keep_elections(old: &OspfState, new: &mut OspfState) {
    // Remembered per segment network: (DR, BDR), only for segments that had neighbours.
    let mut memory: BTreeMap<Ipv4Net, (Option<Ipv4Addr>, Option<Ipv4Addr>)> = BTreeMap::new();
    for ((d, name), seg) in &old.segments {
        if seg.dr.is_none() || !old.neighbors.get(d).into_iter().flatten().any(|n| n.iface == *name) {
            continue;
        }
        if let Some((_, net, _, _)) = old.interfaces.get(d).into_iter().flatten().find(|x| x.0 == *name) {
            memory.insert(net.network(), (seg.dr, seg.bdr));
        }
    }
    if memory.is_empty() {
        return;
    }
    let keys: Vec<(DeviceId, String)> = new.segments.keys().cloned().collect();
    for (d, name) in keys {
        let Some(net) = new.interfaces.get(&d).into_iter().flatten().find(|x| x.0 == name).map(|x| x.1.network()) else { continue };
        let Some((dr0, bdr0)) = memory.get(&net).copied() else { continue };
        let own = crate::ospf::iface_addr(new, d, &name);
        let members: BTreeSet<Ipv4Addr> =
            new.neighbors.get(&d).into_iter().flatten().filter(|n| n.iface == name).map(|n| n.address).chain(own).collect();
        let Some(seg) = new.segments.get_mut(&(d, name.clone())) else { continue };
        if seg.dr.is_none() {
            continue;
        }
        let present = |a: Option<Ipv4Addr>| a.filter(|x| members.contains(x));
        let fresh: Segment = *seg;
        let (dr, bdr) = match (present(dr0), present(bdr0)) {
            (Some(dr), Some(bdr)) if dr != bdr => (Some(dr), Some(bdr)),
            (Some(dr), _) => (Some(dr), [fresh.dr, fresh.bdr].into_iter().flatten().find(|x| *x != dr)),
            (None, Some(bdr)) => (Some(bdr), [fresh.dr, fresh.bdr].into_iter().flatten().find(|x| *x != bdr)),
            (None, None) => (fresh.dr, fresh.bdr),
        };
        seg.dr = dr;
        seg.bdr = bdr;
    }
}

impl crate::Sim {
    /// OSPF's state for the lab, keeping the DR elections already made.
    pub(crate) fn ospf_compute(&self, t: &Topology) -> OspfState {
        let mut st = crate::ospf::compute(t);
        if let Some(old) = &self.ospf {
            keep_elections(old, &mut st);
        }
        st
    }

    /// `clear ip ospf process`: adjacencies form again and every segment elects its DR and BDR
    /// afresh, by the priorities and router IDs configured now.
    pub fn clear_ospf(&mut self) {
        self.ospf = None;
        self.ospf_adjacent.clear();
    }
}
