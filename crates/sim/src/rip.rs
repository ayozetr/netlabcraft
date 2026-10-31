//! RIP (RFC 2453), computed the way the protocol converges, with its timers in simulated time.
//!
//! Interfaces under a `network` statement (classful match) run RIP. Two routers are neighbours
//! when such interfaces (not passive) share a subnet and reach each other at layer 2. Each
//! router starts from its connected networks (0 hops) and, round after round, takes from each
//! neighbour every route one hop further, keeping the fewest hops (ties: lowest next hop) and
//! dropping anything past 15. Split horizon: a router never offers a route back to the neighbour
//! it learned it from (on the wire it offers it with metric 16: poisoned reverse, §3.4.3). A
//! router with `default-information originate` advertises a default route. Both versions behave
//! classless (no automatic summarisation).
//!
//! **Timers** (`timers basic UPDATE INVALID HOLDDOWN FLUSH`, 30/180/180/240 s by default):
//! updates go out every UPDATE seconds. A neighbour that falls silent while the interface
//! towards it stays up (a switch port shut in between, a cable pulled at the far end) keeps its
//! routes until INVALID seconds after its last update; then they are possibly down (metric 16,
//! unusable) until FLUSH seconds after it, and for HOLDDOWN seconds no worse route to those
//! networks is taken. A route that goes away at once (its next hop withdrew it, or the
//! interface towards it went down) is possibly down from then on, likewise. Every change sends
//! a triggered update (§3.10.1) a second later, with withdrawn routes at metric 16.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net, MacAddr};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::control::{self, RipEntry};
use netlabcraft_wire::{Ipv4, PROTO_UDP, Udp};

use crate::{L3If, Sim, l3_ifaces, up_ifaces};

/// Hop count meaning unreachable.
pub const INFINITY: u8 = 16;
/// Most neighbours held, and routes possibly down, at once.
const MAX_HELD: usize = 1_024;
/// Triggered updates leave this long after the change (RFC 2453 asks for 1-5 s).
pub const TRIGGER_DELAY_US: u64 = 1_000_000;

/// A route RIP installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RipRoute {
    pub net: Ipv4Net,
    pub via: Ipv4Addr,
    pub iface: String,
    pub hops: u8,
}

/// A route that went away and is advertised with metric 16 until it is flushed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RipDown {
    pub router: DeviceId,
    pub net: Ipv4Net,
    pub via: Ipv4Addr,
    pub iface: String,
    pub hops: u8,
    /// No worse route to the network is taken before this time (µs).
    pub holddown_until: u64,
    /// Whether the hold-down has run out (and been acted on).
    pub hold_done: bool,
    /// Gone from the table at this time.
    pub flush_at: u64,
}

/// RIP's view of the lab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RipState {
    pub routes: BTreeMap<DeviceId, Vec<RipRoute>>,
    /// Neighbours per router: (their device, their address, our interface).
    pub neighbors: BTreeMap<DeviceId, Vec<(DeviceId, Ipv4Addr, String)>>,
    /// Interfaces running RIP per router: (name, network, passive).
    pub interfaces: BTreeMap<DeviceId, Vec<(String, Ipv4Net, bool)>>,
    /// Routes possibly down per router.
    pub down: BTreeMap<DeviceId, Vec<RipDown>>,
    /// Neighbours no longer heard but still within the invalid time: (router, address, until).
    pub held: Vec<Held>,
}

/// A neighbour no longer heard, kept until the invalid timer runs out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub router: DeviceId,
    pub iface: String,
    pub address: Ipv4Addr,
    pub peer: DeviceId,
    pub until: u64,
}

/// A router's timers in µs: update, invalid, hold-down, flush.
pub fn timers_us(d: &Device) -> (u64, u64, u64, u64) {
    let (u, i, h, f) = d.rip.as_ref().map_or((30, 180, 180, 240), |r| r.timers());
    let us = |s: u32| u64::from(s.max(1)) * 1_000_000;
    (us(u), us(i), us(h), us(f))
}

fn rip_ifaces(t: &Topology, d: &Device) -> Vec<(L3If, bool)> {
    let Some(r) = &d.rip else { return Vec::new() };
    up_ifaces(t, d)
        .into_iter()
        .filter(|i| r.covers(i.net.addr))
        .map(|i| {
            let passive = r.passive.iter().any(|p| netlabcraft_model::ifname::parse(p).is_some_and(|n| n.to_string() == i.name));
            (i, passive)
        })
        .collect()
}

/// One entry of a router's table while computing: hops, and (interface, next hop) unless connected.
type Entry = (u8, Option<(L3If, Ipv4Addr, DeviceId)>);

/// RIP converged over the lab as it is now, every neighbour heard.
pub fn compute(t: &Topology) -> RipState {
    compute_with(t, &[])
}

/// [`compute`] with neighbours that are no longer heard but still within their invalid time.
pub fn compute_with(t: &Topology, held: &[Held]) -> RipState {
    let mut st = RipState::default();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| d.routes_ip() && d.rip.is_some()).collect();
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
            let reach = crate::ospf::iface_reach(t, a, ia);
            for b in &routers {
                if b.id == a.id {
                    continue;
                }
                for (ib, pb) in ifs.get(&b.id).into_iter().flatten() {
                    if !*pb
                        && ib.net.network() == ia.net.network()
                        && (ib.vlan == ia.vlan || ia.port == crate::SVI_PORT || ib.port == crate::SVI_PORT)
                        && crate::ospf::reaches(t, &reach, b, ib)
                    {
                        adj.entry(a.id).or_default().push((ia.clone(), ib.net.addr, b.id));
                        st.neighbors.entry(a.id).or_default().push((b.id, ib.net.addr, ia.name.clone()));
                    }
                }
            }
        }
    }
    // Neighbours still within their invalid time: as if heard.
    for h in held {
        let Some((ia, _)) = ifs.get(&h.router).into_iter().flatten().find(|(i, p)| i.name == h.iface && !*p) else { continue };
        if adj.get(&h.router).is_some_and(|l| l.iter().any(|x| x.0.name == h.iface && x.1 == h.address)) {
            continue;
        }
        adj.entry(h.router).or_default().push((ia.clone(), h.address, h.peer));
        st.neighbors.entry(h.router).or_default().push((h.peer, h.address, h.iface.clone()));
    }
    st.held = held.to_vec();
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

/// The entries a router puts in an update out of one interface: its other connected networks
/// (metric 1), its routes (hops + 1; 16 for those learned through this interface: poisoned
/// reverse) and the routes possibly down (16).
pub fn update_entries(st: &RipState, me: DeviceId, out: &str) -> Vec<RipEntry> {
    let ifs = st.interfaces.get(&me).cloned().unwrap_or_default();
    let mut entries: Vec<RipEntry> =
        ifs.iter().filter(|(n, ..)| n != out).map(|(_, net, _)| RipEntry { net: net.addr, mask: net.mask(), metric: 1 }).collect();
    for r in st.routes.get(&me).into_iter().flatten() {
        let metric = if r.iface == out { u32::from(INFINITY) } else { u32::from(r.hops) + 1 };
        entries.push(RipEntry { net: r.net.addr, mask: r.net.mask(), metric });
    }
    for x in st.down.get(&me).into_iter().flatten() {
        if !entries.iter().any(|e| e.net == x.net.addr && e.mask == x.net.mask()) {
            entries.push(RipEntry { net: x.net.addr, mask: x.net.mask(), metric: u32::from(INFINITY) });
        }
    }
    entries
}

impl Sim {
    /// Brings RIP's state up to date at the current simulated time (see the module notes):
    /// recomputes after the lab changed, keeps silent neighbours until their invalid time,
    /// marks lost routes possibly down with their hold-down, flushes them, and sends the
    /// triggered updates the changes call for.
    pub(crate) fn rip_refresh(&mut self, t: &Topology) {
        let now = self.now;
        if !t.devices.iter().any(|d| d.rip.is_some()) {
            self.rip = Some(RipState::default());
            self.rip_held.clear();
            self.rip_down.clear();
            return;
        }
        let old = self.rip.take();
        self.rip_held.retain(|h| h.until > now);
        let raw = compute(t);
        let heard =
            |st: &RipState, r: DeviceId, iface: &str, a: Ipv4Addr| st.neighbors.get(&r).is_some_and(|l| l.iter().any(|n| n.2 == iface && n.1 == a));
        let iface_up = |st: &RipState, r: DeviceId, iface: &str| st.interfaces.get(&r).is_some_and(|l| l.iter().any(|i| i.0 == iface && !i.2));
        self.rip_held.retain(|h| !heard(&raw, h.router, &h.iface, h.address) && iface_up(&raw, h.router, &h.iface));
        if let Some(old) = &old {
            for (me, list) in &old.neighbors {
                let Some(d) = t.device(*me) else { continue };
                for (peer, addr, iface) in list {
                    if heard(&raw, *me, iface, *addr) || !iface_up(&raw, *me, iface) {
                        continue;
                    }
                    if self.rip_held.iter().any(|h| h.router == *me && h.iface == *iface && h.address == *addr)
                        || old.held.iter().any(|h| h.router == *me && h.iface == *iface && h.address == *addr && h.until <= now)
                    {
                        continue;
                    }
                    // Silent: its routes stay until the invalid time after its last update.
                    let (update, invalid, ..) = timers_us(d);
                    let until = (now - now % update).saturating_add(invalid);
                    if until > now && self.rip_held.len() < MAX_HELD {
                        self.rip_held.push(Held { router: *me, iface: iface.clone(), address: *addr, peer: *peer, until });
                    }
                }
            }
        }
        let mut new = if self.rip_held.is_empty() { raw } else { compute_with(t, &self.rip_held) };
        // Routes lost since the last look are possibly down.
        self.rip_down.retain(|x| x.flush_at > now && t.device(x.router).is_some_and(|d| d.rip.is_some()));
        if let Some(old) = &old {
            for (me, routes) in &old.routes {
                let Some(d) = t.device(*me) else { continue };
                let (_, invalid, holddown, flush) = timers_us(d);
                for r in routes {
                    let still = new.routes.get(me).is_some_and(|l| l.iter().any(|x| x.net == r.net));
                    if still || self.rip_down.iter().any(|x| x.router == *me && x.net == r.net) || self.rip_down.len() >= MAX_HELD {
                        continue;
                    }
                    self.rip_down.push(RipDown {
                        router: *me,
                        net: r.net,
                        via: r.via,
                        iface: r.iface.clone(),
                        hops: r.hops,
                        holddown_until: now.saturating_add(holddown),
                        hold_done: false,
                        flush_at: now.saturating_add(flush.saturating_sub(invalid).max(1)),
                    });
                }
            }
        }
        // Hold-down: no worse route to a network that went down until it runs out.
        for (me, routes) in new.routes.iter_mut() {
            routes.retain(|r| !self.rip_down.iter().any(|x| x.router == *me && x.net == r.net && x.holddown_until > now && r.hops > x.hops));
        }
        // A route that came back ends its possibly-down entry.
        self.rip_down.retain(|x| !new.routes.get(&x.router).is_some_and(|l| l.iter().any(|r| r.net == x.net)));
        for x in &mut self.rip_down {
            x.hold_done = x.holddown_until <= now;
        }
        for x in &self.rip_down {
            new.down.entry(x.router).or_default().push(x.clone());
        }
        new.held = self.rip_held.clone();
        if let Some(old) = &old {
            self.rip_triggered(t, old, &new);
        }
        self.rip = Some(new);
    }

    /// When RIP's next timer runs out: a silent neighbour's invalid time, a hold-down or a flush.
    pub(crate) fn rip_next_expiry(&self) -> Option<u64> {
        let held = self.rip_held.iter().map(|h| h.until);
        let down = self.rip_down.iter().map(|x| if x.hold_done { x.flush_at } else { x.holddown_until.min(x.flush_at) });
        held.chain(down).min()
    }

    /// Triggered updates (RFC 2453 §3.10.1): each router whose routes changed sends, a second
    /// later, the changed entries out of each RIP interface with neighbours (withdrawn ones,
    /// and those learned through that interface, at metric 16).
    fn rip_triggered(&mut self, t: &Topology, old: &RipState, new: &RipState) {
        let at = self.now.saturating_add(TRIGGER_DELAY_US);
        let mut out: Vec<(DeviceId, String, Vec<RipEntry>)> = Vec::new();
        for (me, ifs) in &new.interfaces {
            // A router just starting sends its whole table with the regular updates.
            let known_before = old.interfaces.contains_key(me);
            for (name, _, passive) in ifs {
                if *passive || !new.neighbors.get(me).is_some_and(|l| l.iter().any(|n| n.2 == *name)) {
                    continue;
                }
                let was: BTreeMap<(Ipv4Addr, Ipv4Addr), u32> =
                    update_entries(old, *me, name).into_iter().map(|e| ((e.net, e.mask), e.metric)).collect();
                let now_entries = update_entries(new, *me, name);
                let mut changed: Vec<RipEntry> = now_entries.iter().filter(|e| was.get(&(e.net, e.mask)) != Some(&e.metric)).copied().collect();
                for ((net, mask), _) in was.iter().filter(|(k, _)| !now_entries.iter().any(|e| (e.net, e.mask) == **k)) {
                    changed.push(RipEntry { net: *net, mask: *mask, metric: u32::from(INFINITY) });
                }
                if !changed.is_empty() && known_before {
                    out.push((*me, name.clone(), changed));
                }
            }
        }
        for (me, name, entries) in out {
            let Some(d) = t.device(me) else { continue };
            self.rip_send(t, d, &name, &entries, at);
        }
    }

    /// Sends RIP responses with `entries` out of an interface (as many as they take).
    pub(crate) fn rip_send(&mut self, t: &Topology, d: &Device, iface: &str, entries: &[RipEntry], at: u64) {
        let Some(r) = &d.rip else { return };
        let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == iface) else { return };
        let dst = if r.version >= 2 { control::RIP_ROUTERS } else { Ipv4Addr::BROADCAST };
        let mac = if r.version >= 2 { control::ipv4_multicast_mac(dst) } else { MacAddr::BROADCAST };
        for chunk in entries.chunks(control::RIP_MAX_ENTRIES).take(64) {
            let udp = Udp { src_port: control::RIP_PORT, dst_port: control::RIP_PORT, payload: control::rip_encode(r.version, chunk) };
            let ip = Ipv4 { id: 0, ttl: 1, protocol: PROTO_UDP, src: ifc.net.addr, dst, payload: udp.encode(ifc.net.addr, dst) };
            self.queue_l3(t, d, &ifc, ifc.mac, mac, ip.encode(), at);
        }
    }
}
