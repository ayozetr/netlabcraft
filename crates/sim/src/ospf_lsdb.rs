//! What OSPF's link-state database holds once the protocol has converged (RFC 2328 §12.4),
//! derived from the computed [`OspfState`]: a router-LSA per router and area, a network-LSA per
//! segment with a designated router, summary-LSAs from the area border routers, and
//! AS-external-LSAs for redistributed and default routes, and in a not-so-stubby area the type 7
//! LSAs its routers originate (translated to type 5 by the area's border router). Stub areas and
//! NSSAs get no type 4 or 5 LSAs and, with `no-summary`, a default summary instead of the rest.
//! Each LSA carries a sequence number and an age (see [`Stamp`]). Also the memory of past DR
//! elections, which makes the election non-preemptive as the standard has it, and
//! `clear ip ospf process`.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::Topology;

use crate::ospf::{AreaKind, OspfState, Segment, area_kind, backbone_attached};

/// The kinds of LSA `show ip ospf database` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LsaKind {
    Router = 1,
    Network = 2,
    Summary = 3,
    /// A summary-LSA for an AS boundary router (type 4).
    AsbrSummary = 4,
    External = 5,
    /// An AS-external LSA that stays inside one not-so-stubby area (type 7).
    Nssa = 7,
}

/// What a link in a router-LSA connects to (RFC 2328 §12.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkKind {
    PointToPoint,
    Transit,
    Stub,
}

/// One link of a router-LSA.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LsaLink {
    pub kind: LinkKind,
    /// The neighbour's router ID (point to point), the DR's address (transit), the network (stub).
    pub id: Ipv4Addr,
    /// This router's interface address (point to point and transit), the mask (stub).
    pub data: Ipv4Addr,
    pub metric: u32,
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
    /// Router-LSA: the links themselves.
    pub links: Vec<LsaLink>,
    /// Summary and external LSAs: the metric the originator advertises.
    pub metric: u32,
    /// Network-LSA: the routers on the segment (the DR included).
    pub attached: Vec<Ipv4Addr>,
    /// External and NSSA LSAs: a type 1 metric (the cost to the advertiser is added), not type 2.
    pub type1: bool,
    /// NSSA LSAs: the border router that translates it into a type 5 LSA, when one does.
    pub translator: Option<Ipv4Addr>,
    /// The sequence number: 0x80000001 when first originated, one more each time the router
    /// originates the LSA again (see [`Stamp`]).
    pub seq: u32,
    /// Seconds since it was last originated (or refreshed).
    pub age: u16,
    /// External LSA: the route tag it carries (0: none).
    pub tag: u32,
}

/// The first sequence number of an LSA (RFC 2328 §12.1.6).
pub const INITIAL_SEQ: u32 = 0x8000_0001;
/// A router originates an LSA again after this many seconds (LSRefreshTime, RFC 2328 Appendix B).
pub const LS_REFRESH_SECS: u64 = 1800;
/// The age at which an LSA is flushed (MaxAge); an LSA the originator keeps refreshing never gets there.
pub const MAX_AGE_SECS: u16 = 3600;
/// Most LSAs whose history is kept.
const MAX_STAMPS: usize = 8192;

impl Lsa {
    fn new(kind: LsaKind, area: Option<u32>, id: Ipv4Addr, adv: Ipv4Addr, count: u32) -> Lsa {
        Lsa {
            kind,
            area,
            id,
            adv,
            count,
            links: Vec::new(),
            metric: 0,
            attached: Vec::new(),
            type1: false,
            translator: None,
            seq: INITIAL_SEQ,
            age: 0,
            tag: 0,
        }
    }

    /// What identifies an LSA in the database: its type, area, link-state ID and advertising router.
    fn key(&self) -> (LsaKind, Option<u32>, Ipv4Addr, Ipv4Addr) {
        (self.kind, self.area, self.id, self.adv)
    }

    /// A hash of what the LSA says (not its sequence number or age): when it changes the
    /// originator sends the LSA again.
    fn digest(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.kind, self.area, self.id, self.adv, self.count, &self.links, self.metric, &self.attached, self.type1, self.translator, self.tag)
            .hash(&mut h);
        h.finish()
    }
}

/// What the simulator remembers of one LSA to give it a sequence number and an age.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// The sequence number of the last origination.
    seq: u32,
    /// When it was originated, in simulated microseconds.
    born: u64,
    /// What it said then.
    digest: u64,
    /// Whether the LSA is in the database now. One that left keeps its number, so it carries on
    /// from it when it comes back.
    present: bool,
    /// Whether it was in the database the time before (while the history is being brought up to date,
    /// `present` is the new answer).
    present_before: bool,
    /// What the LSA said when it was last in the database (a withdrawn LSA is flushed from
    /// this).
    body: Option<Lsa>,
    /// When it left the database, in simulated microseconds.
    gone: Option<u64>,
}

/// What identifies an LSA in the database: its type, area, link-state ID and advertising router.
pub type LsaKey = (LsaKind, Option<u32>, Ipv4Addr, Ipv4Addr);

/// What a change of the lab did to an LSA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StampEvent {
    /// The advertising router originated it (again), with this sequence number; `first` for the
    /// first time.
    Originated { key: LsaKey, seq: u32, first: bool },
    /// The advertising router withdrew it: it is flushed from the other routers' databases.
    Withdrawn { key: LsaKey },
}

impl Stamp {
    /// The sequence number as of `now`: the originator sends the LSA again every [`LS_REFRESH_SECS`].
    fn seq_at(&self, now: u64) -> u32 {
        let secs = now.saturating_sub(self.born) / 1_000_000;
        self.seq.saturating_add(u32::try_from(secs / LS_REFRESH_SECS).unwrap_or(u32::MAX))
    }
}

/// The sequence numbers and ages of the LSAs of a lab, keyed by what identifies each LSA.
pub type Stamps = BTreeMap<(LsaKind, Option<u32>, Ipv4Addr, Ipv4Addr), Stamp>;

/// Brings the history up to date with the LSAs in the lab now, at simulated time `now`: a new
/// LSA starts at 0x80000001, one whose content changed is originated again (the number goes up
/// and the age restarts), one that left is remembered as gone.
pub fn restamp(stamps: &mut Stamps, now: u64, lsas: &[Lsa]) -> Vec<StampEvent> {
    let mut events = Vec::new();
    for s in stamps.values_mut() {
        s.present = false;
    }
    for l in lsas {
        let digest = l.digest();
        match stamps.get_mut(&l.key()) {
            Some(s) if s.digest == digest && s.present_before => {
                s.present = true;
            }
            Some(s) => {
                // Originated again: the next number after the refreshes it has had since.
                s.seq = s.seq_at(now).wrapping_add(1).max(INITIAL_SEQ);
                s.born = now;
                s.digest = digest;
                s.present = true;
                s.body = Some(l.clone());
                s.gone = None;
                events.push(StampEvent::Originated { key: l.key(), seq: s.seq, first: false });
            }
            None => {
                let stamp = Stamp { seq: INITIAL_SEQ, born: now, digest, present: true, present_before: false, body: Some(l.clone()), gone: None };
                stamps.insert(l.key(), stamp);
                events.push(StampEvent::Originated { key: l.key(), seq: INITIAL_SEQ, first: true });
            }
        }
    }
    for (k, s) in stamps.iter_mut() {
        if s.present_before && !s.present {
            s.gone = Some(now);
            events.push(StampEvent::Withdrawn { key: *k });
        }
        s.present_before = s.present;
    }
    if stamps.len() > MAX_STAMPS {
        stamps.retain(|_, s| s.present);
    }
    events
}

/// The sequence number the router-LSA of router `rid` has in `area` as of `now`, for the database
/// exchange on the wire (0x80000001 while it has not been seen yet).
pub fn router_lsa_seq(stamps: &Stamps, now: u64, area: u32, rid: Ipv4Addr) -> u32 {
    stamps.get(&(LsaKind::Router, Some(area), rid, rid)).map_or(INITIAL_SEQ, |s| s.seq_at(now))
}

/// Gives the LSAs their sequence number and age as of `now`: the originator sends an LSA again
/// every [`LS_REFRESH_SECS`] with the next number, so the age counts from the last of those.
pub fn apply_stamps(stamps: &Stamps, now: u64, lsas: &mut [Lsa]) {
    for l in lsas {
        let Some(s) = stamps.get(&l.key()) else { continue };
        let secs = now.saturating_sub(s.born) / 1_000_000;
        l.seq = s.seq_at(now);
        l.age = u16::try_from(secs % LS_REFRESH_SECS).unwrap_or(0).min(MAX_AGE_SECS);
    }
}

/// The metric an area border router advertises for a network: the cost of its own interface
/// when it is attached, else the cost of its route to it.
fn metric_to(st: &OspfState, abr: DeviceId, net: &Ipv4Net) -> u32 {
    st.interfaces
        .get(&abr)
        .into_iter()
        .flatten()
        .find(|x| x.1.network() == net.network())
        .map(|x| x.2)
        .or_else(|| st.routes.get(&abr).into_iter().flatten().find(|r| r.net == net.network()).map(|r| r.cost))
        .unwrap_or(1)
}

/// What an area border router advertises as the metric to an AS boundary router: the cost of its
/// path to it through an area both are in (other than `into`, the area being told). A router on a
/// segment the border router is on costs the border router's interface; a farther one, the cost
/// of the route to one of its networks less the cost of its own interface on that network.
fn asbr_distance(st: &OspfState, abr: DeviceId, asbr: DeviceId, into: u32, abr_areas: &BTreeSet<u32>) -> Option<u32> {
    let mut best: Option<u32> = None;
    for (name, net, cost, _) in st.interfaces.get(&asbr).into_iter().flatten() {
        let Some(area) = st.segments.get(&(asbr, name.clone())).map(|s| s.area) else { continue };
        if area == into || !abr_areas.contains(&area) {
            continue;
        }
        let shared = st
            .interfaces
            .get(&abr)
            .into_iter()
            .flatten()
            .find(|x| x.1.network() == net.network() && st.segments.get(&(abr, x.0.clone())).is_some_and(|s| s.area == area))
            .map(|x| x.2);
        let farther = || st.routes.get(&abr).into_iter().flatten().find(|r| r.net == net.network()).map(|r| r.cost.saturating_sub(*cost).max(1));
        if let Some(m) = shared.or_else(farther) {
            best = Some(best.map_or(m, |b| b.min(m)));
        }
    }
    best
}

/// The routers whose advertisements reach `me` by flooding: itself and every router joined to
/// it through a chain of adjacencies. A router with no adjacency back to it has aged out.
fn flooding_domain(st: &OspfState, me: DeviceId) -> BTreeSet<DeviceId> {
    let mut seen = BTreeSet::from([me]);
    let mut todo = vec![me];
    while let Some(d) = todo.pop() {
        for n in st.neighbors.get(&d).into_iter().flatten() {
            if seen.insert(n.device) {
                todo.push(n.device);
            }
        }
    }
    seen
}

/// The database a router holds: the LSAs of every area it has an interface in, and the
/// external ones. Only routers that flooding reaches count: one cut off from this router by a
/// missing adjacency has no LSA here any more (its advertisements age out).
pub fn database(t: &Topology, st: &OspfState, me: DeviceId) -> Vec<Lsa> {
    let flood = flooding_domain(st, me);
    let my_areas: BTreeSet<u32> = st.segments.iter().filter(|((d, _), _)| *d == me).map(|(_, s)| s.area).collect();
    let rid = |d: DeviceId| st.router_ids.get(&d).copied();
    let kind_at = |d: DeviceId, a: u32| t.device(d).map_or(AreaKind::Normal, |x| area_kind(x, a));
    // A router wholly inside stub areas and NSSAs holds no type 4 or 5 LSAs.
    let closed_viewer = !my_areas.is_empty() && my_areas.iter().all(|a| kind_at(me, *a) != AreaKind::Normal);
    let heard = |d: DeviceId, iface: &str| st.neighbors.get(&d).into_iter().flatten().filter(|n| n.iface == iface).count();
    let mut out: BTreeSet<Lsa> = BTreeSet::new();
    // Areas each router is in, and the networks of each area.
    let mut areas_of: BTreeMap<DeviceId, BTreeSet<u32>> = BTreeMap::new();
    let mut nets_in: BTreeMap<u32, BTreeSet<Ipv4Net>> = BTreeMap::new();
    for ((d, iface), seg) in st.segments.iter().filter(|((d, _), _)| flood.contains(d)) {
        areas_of.entry(*d).or_default().insert(seg.area);
        if let Some((_, net, _, _)) = st.interfaces.get(d).into_iter().flatten().find(|x| x.0 == *iface) {
            nets_in.entry(seg.area).or_default().insert(*net);
        }
    }
    // The external routes: type 5 LSAs as (advertiser, owner of the metric, network), and type 7
    // LSAs as (area, advertiser, network, owner, whether the translator sends it on). A router with an interface outside any NSSA
    // originates type 5 as before; one only in NSSAs originates type 7 into each, and the
    // area's translator sends it on as type 5.
    let mut type5: Vec<(DeviceId, DeviceId, Ipv4Net)> = Vec::new();
    let mut type7: Vec<(u32, DeviceId, Ipv4Net, DeviceId, bool)> = Vec::new();
    for (x, nets) in st.externals.iter().filter(|(x, _)| flood.contains(x)) {
        let xa: Vec<u32> = areas_of.get(x).map(|s| s.iter().copied().collect()).unwrap_or_default();
        let all_nssa: Vec<u32> = xa.iter().copied().filter(|a| kind_at(*x, *a) == AreaKind::Nssa).collect();
        let outside = xa.len() > all_nssa.len() || xa.is_empty();
        // A border router set to `no-redistribution` keeps its own routes out of the NSSA.
        let xo = t.device(*x).and_then(|d| d.ospf.as_ref());
        let nssa_areas: Vec<u32> = all_nssa.into_iter().filter(|a| !(outside && xo.is_some_and(|o| o.area(*a).nssa_no_redist))).collect();
        for n in nets {
            if outside {
                type5.push((*x, *x, *n));
            }
            for a in &nssa_areas {
                type7.push((*a, *x, *n, *x, !outside));
                if !outside && let Some(tr) = st.translators.get(a).filter(|tr| flood.contains(*tr)) {
                    type5.push((*tr, *x, *n));
                }
            }
        }
    }
    type5.sort();
    type5.dedup();
    for area in &my_areas {
        for (d, areas) in areas_of.iter().filter(|(_, a)| a.contains(area)) {
            let Some(r) = rid(*d) else { continue };
            // Router-LSA: point-to-point links count twice (the neighbour and the subnet), a
            // segment with neighbours once (its transit network), anything else once (a stub).
            let mut links: Vec<LsaLink> = Vec::new();
            for (name, net, cost, _) in st.interfaces.get(d).into_iter().flatten() {
                let Some(seg) = st.segments.get(&(*d, name.clone())).filter(|s| s.area == *area) else { continue };
                let mine = st.addresses.get(&(*d, name.clone())).copied().unwrap_or(net.addr);
                let stub = LsaLink { kind: LinkKind::Stub, id: net.network().addr, data: net.mask(), metric: *cost };
                if seg.dr.is_none() {
                    for n in st.neighbors.get(d).into_iter().flatten().filter(|n| n.iface == *name) {
                        links.push(LsaLink { kind: LinkKind::PointToPoint, id: n.router_id, data: mine, metric: *cost });
                    }
                    links.push(stub);
                } else if let (Some(dr), true) = (seg.dr, heard(*d, name) > 0) {
                    links.push(LsaLink { kind: LinkKind::Transit, id: dr, data: mine, metric: *cost });
                } else {
                    links.push(stub);
                }
            }
            let mut lsa = Lsa::new(LsaKind::Router, Some(*area), r, r, u32::try_from(links.len()).unwrap_or(u32::MAX));
            lsa.links = links;
            out.insert(lsa);
            // Network-LSAs: from the DR of each segment that has one and neighbours on it.
            for ((dev, name), seg) in st.segments.iter().filter(|((dev, _), s)| dev == d && s.area == *area) {
                if let (Some(dr), true) = (seg.dr, heard(*dev, name) > 0) {
                    let owner = owner_of(t, st, dr).and_then(rid);
                    if let Some(adv) = owner {
                        let prefix = st.interfaces.get(dev).into_iter().flatten().find(|x| x.0 == *name).map_or(0, |x| x.1.prefix);
                        let mut lsa = Lsa::new(LsaKind::Network, Some(*area), dr, adv, u32::from(prefix));
                        let mut members: BTreeSet<Ipv4Addr> =
                            st.neighbors.get(dev).into_iter().flatten().filter(|n| n.iface == *name).map(|n| n.router_id).collect();
                        members.extend(rid(*dev));
                        lsa.attached = members.into_iter().collect();
                        out.insert(lsa);
                    }
                }
            }
            // ASBR-summary-LSAs (§12.4.3): an area border router tells this area how to reach an
            // AS boundary router that sits in another of its areas. None go into a stub area or
            // an NSSA, which have no type 5 LSAs to need them.
            let abr_ok = areas.len() > 1 && backbone_attached(st, *d);
            let into = kind_at(*d, *area);
            if abr_ok && into == AreaKind::Normal {
                for asbr in type5.iter().map(|x| x.0).collect::<BTreeSet<DeviceId>>().into_iter().filter(|a| *a != *d) {
                    let Some(ar) = rid(asbr) else { continue };
                    let Some(dist) = asbr_distance(st, *d, asbr, *area, areas) else { continue };
                    let mut lsa = Lsa::new(LsaKind::AsbrSummary, Some(*area), ar, r, 0);
                    lsa.metric = dist;
                    out.insert(lsa);
                }
            }
            // Summary-LSAs: an area border router tells this area about the others' networks. With
            // `no-summary` a stub area or NSSA gets only a default route in their place.
            if abr_ok {
                let conf = t.device(*d).and_then(|x| x.ospf.as_ref()).map(|o| o.area(*area)).unwrap_or_default();
                if into != AreaKind::Normal && (into == AreaKind::Stub || conf.no_summary) {
                    let mut lsa = Lsa::new(LsaKind::Summary, Some(*area), Ipv4Addr::UNSPECIFIED, r, 0);
                    lsa.metric = conf.default_cost.unwrap_or(1);
                    out.insert(lsa);
                }
                if !(into != AreaKind::Normal && conf.no_summary) {
                    for (other, nets) in nets_in.iter().filter(|(a, _)| *a != area) {
                        let _ = other;
                        for n in nets {
                            if nets_in.get(area).is_some_and(|mine| mine.contains(n)) {
                                continue;
                            }
                            let mut lsa = Lsa::new(LsaKind::Summary, Some(*area), n.addr, r, u32::from(n.prefix));
                            lsa.metric = metric_to(st, *d, n);
                            out.insert(lsa);
                        }
                    }
                }
                // The NSSA's default route as a type 7 LSA, when asked for and not replaced by the
                // type 3 default of `no-summary`.
                if into == AreaKind::Nssa && conf.nssa_default && !conf.no_summary {
                    let mut lsa = Lsa::new(LsaKind::Nssa, Some(*area), Ipv4Addr::UNSPECIFIED, r, 0);
                    lsa.metric = 1;
                    out.insert(lsa);
                }
            }
        }
        // Type 7 LSAs of the routers in this NSSA (the viewer is in the area too).
        if kind_at(me, *area) == AreaKind::Nssa {
            for (_, adv, net, owner, translated) in type7.iter().filter(|x| x.0 == *area) {
                let Some(ar) = rid(*adv) else { continue };
                let mut lsa = Lsa::new(LsaKind::Nssa, Some(*area), net.addr, ar, u32::from(net.prefix));
                lsa.metric = st.external_metrics.get(&(*owner, *net)).map_or(if net.prefix == 0 { 1 } else { 20 }, |m| m.0);
                lsa.type1 = st.external_metrics.get(&(*owner, *net)).is_some_and(|m| m.1);
                lsa.tag = st.external_tags.get(&(*owner, *net)).copied().unwrap_or(0);
                // The default route is not translated; the rest is, by the area's border router.
                lsa.translator = if net.prefix == 0 || !*translated { None } else { st.translators.get(area).and_then(|tr| rid(*tr)) };
                out.insert(lsa);
            }
        }
    }
    if !closed_viewer {
        for (adv, owner, n) in &type5 {
            let Some(r) = rid(*adv) else { continue };
            let mut lsa = Lsa::new(LsaKind::External, None, n.addr, r, u32::from(n.prefix));
            let m = st.external_metrics.get(&(*owner, *n));
            lsa.metric = m.map_or(if n.prefix == 0 { 1 } else { 20 }, |m| m.0);
            lsa.type1 = m.is_some_and(|m| m.1);
            lsa.tag = st.external_tags.get(&(*owner, *n)).copied().unwrap_or(0);
            out.insert(lsa);
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

/// Simulated microseconds one hop of flooding takes: a new instance of an LSA reaches a router
/// this long after its originator, per router on the way (the LSU, the acknowledgement and the
/// neighbour's own flooding).
pub const FLOOD_HOP_US: u64 = 10_000;
/// How long a withdrawn LSA stays in the other routers' databases at MaxAge before every router has
/// acknowledged the flush and removed it.
pub const FLUSH_US: u64 = 2_000_000;

/// How many adjacencies away each router is from `me`, counting only the adjacencies LSAs of
/// `area` travel over (every one for an AS-external LSA).
fn flood_distances(st: &OspfState, me: DeviceId, area: Option<u32>) -> BTreeMap<DeviceId, u32> {
    let mut dist = BTreeMap::from([(me, 0u32)]);
    let mut todo = std::collections::VecDeque::from([me]);
    while let Some(d) = todo.pop_front() {
        let here = dist.get(&d).copied().unwrap_or(0);
        for n in st.neighbors.get(&d).into_iter().flatten() {
            let in_area = area.is_none_or(|a| st.segments.get(&(d, n.iface.clone())).is_some_and(|s| s.area == a));
            if in_area && !dist.contains_key(&n.device) {
                dist.insert(n.device, here.saturating_add(1));
                todo.push_back(n.device);
            }
        }
    }
    dist
}

/// Turns the originators' LSAs into the copies router `me` holds: a copy that came over `h`
/// adjacencies is `h` seconds older (every hop adds the transmit delay, RFC 2328 §13.3), the
/// router still holds the previous instance of an LSA that was originated again until the new
/// one has been flooded to it, and an LSA that was withdrawn is held at MaxAge for a moment
/// before every router removes it (the flush, RFC 2328 §14.1).
fn copy_for(st: &OspfState, stamps: &Stamps, now: u64, me: DeviceId, lsas: &mut Vec<Lsa>) {
    let by_rid: BTreeMap<Ipv4Addr, DeviceId> = st.router_ids.iter().map(|(d, r)| (*r, *d)).collect();
    let mut dists: BTreeMap<Option<u32>, BTreeMap<DeviceId, u32>> = BTreeMap::new();
    for l in lsas.iter_mut() {
        let Some(adv) = by_rid.get(&l.adv).copied().filter(|a| *a != me) else { continue };
        let d = dists.entry(l.area).or_insert_with(|| flood_distances(st, me, l.area));
        let Some(h) = d.get(&adv).copied() else { continue };
        l.age = l.age.saturating_add(u16::try_from(h).unwrap_or(u16::MAX)).min(MAX_AGE_SECS - 1);
        let born = stamps.get(&l.key()).map_or(0, |s| s.born);
        if l.seq > INITIAL_SEQ && now < born.saturating_add(u64::from(h) * FLOOD_HOP_US) {
            l.seq -= 1;
        }
    }
    let my_areas: BTreeSet<u32> = st.segments.iter().filter(|((d, _), _)| *d == me).map(|(_, s)| s.area).collect();
    let domain = flooding_domain(st, me);
    for (key, s) in stamps.iter().filter(|(_, s)| !s.present) {
        let (Some(gone), Some(body)) = (s.gone, &s.body) else { continue };
        let Some(adv) = by_rid.get(&key.3).copied().filter(|a| *a != me && domain.contains(a)) else { continue };
        if key.1.is_some_and(|a| !my_areas.contains(&a)) || lsas.iter().any(|l| l.key() == *key) {
            continue;
        }
        let d = dists.entry(key.1).or_insert_with(|| flood_distances(st, me, key.1));
        let Some(h) = d.get(&adv).copied() else { continue };
        let reached = gone.saturating_add(u64::from(h) * FLOOD_HOP_US);
        if now >= reached && now < gone.saturating_add(FLUSH_US) {
            let mut flushed = body.clone();
            flushed.seq = s.seq_at(gone);
            flushed.age = MAX_AGE_SECS;
            lsas.push(flushed);
        }
    }
    lsas.sort();
}

fn router_digest(lsas: &[Lsa]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut each: Vec<(LsaKey, u64)> = lsas.iter().map(|l| (l.key(), l.digest())).collect();
    each.sort();
    each.hash(&mut h);
    h.finish()
}

impl crate::Sim {
    /// The LSAs `me` holds, with the sequence number and age each has now: its own copies, not
    /// the originators' (see [`copy_for`]).
    pub fn lsdb(&mut self, t: &Topology, me: DeviceId) -> Vec<Lsa> {
        let st = self.ospf_state(t);
        let mut lsas = database(t, &st, me);
        apply_stamps(&self.lsa_stamps, self.now, &mut lsas);
        copy_for(&st, &self.lsa_stamps, self.now, me, &mut lsas);
        lsas
    }

    /// Records, for every router, what its database holds now: a new LSA, or one that changed
    /// since the last time, is originated (a new sequence number, age zero) at the current
    /// simulated time. Called each time OSPF's state is recomputed, so each change of the lab is
    /// one re-origination, however long it is before anyone looks. A router whose database
    /// changed runs SPF (counted, and throttled by `timers throttle spf`), and the originations
    /// and floods are reported to the routers that have `debug ip ospf lsa-generation` or
    /// `flood` on.
    pub(crate) fn ospf_restamp(&mut self, t: &Topology) {
        let Some(st) = self.ospf.take() else { return };
        let mut all: BTreeMap<_, Lsa> = BTreeMap::new();
        let mut digests: BTreeMap<DeviceId, u64> = BTreeMap::new();
        let mut holders: BTreeMap<LsaKey, Vec<DeviceId>> = BTreeMap::new();
        for d in st.router_ids.keys() {
            let db = database(t, &st, *d);
            digests.insert(*d, router_digest(&db));
            for l in db {
                holders.entry(l.key()).or_default().push(*d);
                all.entry(l.key()).or_insert(l);
            }
        }
        let list: Vec<Lsa> = all.into_values().collect();
        let events = restamp(&mut self.lsa_stamps, self.now, &list);
        self.ospf_report(t, &st, &events, &holders);
        for (d, digest) in digests {
            if self.ospf_db_hash.get(&d) != Some(&digest) {
                self.ospf_db_hash.insert(d, digest);
                self.spf_scheduled(t, d);
            }
        }
        self.ospf = Some(st);
    }

    /// A router's database changed: it runs SPF. With `timers throttle spf START HOLD MAX` the run
    /// waits START ms; one that follows another within the hold interval waits that long, and the
    /// hold interval doubles each time up to MAX (RFC 2328 leaves the pacing to the vendor, so
    /// this is a model of ours).
    fn spf_scheduled(&mut self, t: &Topology, d: DeviceId) {
        let Some(conf) = t.device(d).and_then(|x| x.ospf.as_ref()) else { return };
        let (start, hold, max) = conf.spf_throttle.unwrap_or((SPF_START_MS, SPF_HOLD_MS, SPF_MAX_MS));
        let now = self.now;
        let (events, wait) = {
            let s = self.st(d);
            let recent = s.spf_last_us.is_some_and(|last| now.saturating_sub(last) < u64::from(s.spf_hold_ms.max(hold)) * 1_000);
            let wait = if recent { s.spf_hold_ms.max(hold) } else { start };
            s.spf_hold_ms = if recent { s.spf_hold_ms.max(hold).saturating_mul(2).min(max.max(hold)) } else { hold };
            s.spf_runs = s.spf_runs.saturating_add(1);
            s.spf_last_us = Some(now);
            (s.debug_ospf_events, wait)
        };
        if events {
            let runs = self.state(d).map_or(0, |s| s.spf_runs);
            self.log(d, &format!("OSPF event: the database changed, SPF run {runs} scheduled in {wait} ms"));
        }
    }

    fn ospf_report(&mut self, t: &Topology, st: &OspfState, events: &[StampEvent], holders: &BTreeMap<LsaKey, Vec<DeviceId>>) {
        if !self.state.values().any(|s| s.debug_ospf_lsa || s.debug_ospf_flood) {
            return;
        }
        let by_rid: BTreeMap<Ipv4Addr, DeviceId> = st.router_ids.iter().map(|(d, r)| (*r, *d)).collect();
        for ev in events {
            let (key, what) = match ev {
                StampEvent::Originated { key, seq, first } => {
                    (key, if *first { format!("originated, sequence 0x{seq:08x}") } else { format!("originated again, sequence 0x{seq:08x}") })
                }
                StampEvent::Withdrawn { key } => (key, "withdrawn: flushed with the age MaxAge".to_string()),
            };
            let title =
                format!("{} LSA {} advertised by {}{}", kind_name(key.0), key.2, key.3, key.1.map_or(String::new(), |a| format!(" in area {a}")));
            let Some(origin) = by_rid.get(&key.3).copied() else { continue };
            if self.state(origin).is_some_and(|s| s.debug_ospf_lsa) {
                self.log(origin, &format!("OSPF lsa-generation: {title} {what}"));
            }
            let reached = holders.get(key).cloned().unwrap_or_default();
            let recipients: Vec<DeviceId> = st.router_ids.keys().copied().filter(|d| *d != origin).collect();
            if self.state(origin).is_some_and(|s| s.debug_ospf_flood) {
                let out = st.neighbors.get(&origin).into_iter().flatten().map(|n| n.iface.clone()).collect::<BTreeSet<_>>().len();
                self.log(origin, &format!("OSPF flood: {title}: sent in an update out of {out} interface{}", if out == 1 { "" } else { "s" }));
            }
            for d in recipients {
                if !self.state(d).is_some_and(|s| s.debug_ospf_flood) {
                    continue;
                }
                let dist = flood_distances(st, d, key.1).get(&origin).copied();
                let Some(h) = dist else { continue };
                let msg = match ev {
                    StampEvent::Originated { .. } if reached.contains(&d) => {
                        format!(
                            "OSPF flood: {title} received, {h} adjacenc{} from its router; installed and acknowledged",
                            if h == 1 { "y" } else { "ies" }
                        )
                    }
                    StampEvent::Withdrawn { .. } => format!("OSPF flood: {title} received with MaxAge; removed from the database once acknowledged"),
                    StampEvent::Originated { .. } => continue,
                };
                self.log(d, &msg);
            }
        }
        let _ = t;
    }

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
        // The process starts over: every router originates its LSAs afresh and runs SPF again.
        self.lsa_stamps.clear();
        self.ospf_db_hash.clear();
        for s in self.state.values_mut() {
            s.spf_runs = 0;
            s.spf_last_us = None;
            s.spf_hold_ms = 0;
        }
    }
}

/// The SPF delays when `timers throttle spf` is not set: milliseconds before the first run, the
/// hold between runs and the longest wait.
pub const SPF_START_MS: u32 = 50;
pub const SPF_HOLD_MS: u32 = 200;
pub const SPF_MAX_MS: u32 = 5_000;

/// The name of an LSA type in the debug output.
fn kind_name(k: LsaKind) -> &'static str {
    match k {
        LsaKind::Router => "router",
        LsaKind::Network => "network",
        LsaKind::Summary => "summary",
        LsaKind::AsbrSummary => "ASBR-summary",
        LsaKind::External => "external",
        LsaKind::Nssa => "NSSA-external",
    }
}
