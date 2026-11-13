//! RIPng (RFC 2080): RIP for IPv6, computed from the topology like RIP (`rip`).
//!
//! A router runs it with `ipv6 router rip NAME` (and `ipv6 unicast-routing`), and interfaces
//! join with `ipv6 rip NAME enable`. Neighbours meet over link-local addresses on a shared
//! link; each router advertises the global prefixes of its RIPng interfaces and takes from each
//! neighbour every prefix one hop further, keeping the fewest hops (ties: lowest next hop),
//! never past 15; split horizon as in RIP. Routes lead to the neighbour's link-local address,
//! with RIPng's metric (hops + 1: a neighbour's own prefix is 2). Responses go to `FF02::9`,
//! UDP port 521, every UPDATE seconds (`control`), with poisoned reverse.
//!
//! **Timers** (`timers basic UPDATE INVALID HOLDDOWN FLUSH`, 30/180/180/300 s by default; RFC
//! 2080 §2.3 and §2.4.2), the same machinery as RIPv2's (`rip`), in simulated time. A neighbour
//! that falls silent while the interface towards it stays up keeps its routes until INVALID
//! seconds after its last response; then they are possibly down (metric 16, unusable) until
//! FLUSH seconds after it, and for HOLDDOWN seconds no worse route to those prefixes is taken.
//! A route that goes away at once (its next hop withdrew it, or the interface went down) is
//! possibly down from then on, likewise. Every change sends a triggered response (§2.5.1) a
//! second later with the changed prefixes, withdrawn ones at metric 16.

use std::collections::BTreeMap;
use std::net::Ipv6Addr;

use netlabcraft_core::{DeviceId, Ipv6Net};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::ripng::{self, RIPNG_PORT, RIPNG_ROUTERS, RipngEntry};

use crate::v6::{L3If6, conf6, routes6, up_ifaces6};
use crate::{SVI_PORT, Sim};

/// Hop count meaning unreachable.
pub const INFINITY: u8 = 16;
/// Most neighbours held, and routes possibly down, at once.
const MAX_HELD: usize = 1_024;
/// Triggered responses leave this long after the change (RFC 2080 asks for 1-5 s).
pub const TRIGGER_DELAY_US: u64 = 1_000_000;

/// A route RIPng installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rip6Route {
    pub net: Ipv6Net,
    pub via: Ipv6Addr,
    pub iface: String,
    /// RIPng's metric: hops + 1.
    pub metric: u8,
}

/// A route that went away and is advertised with metric 16 until it is flushed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rip6Down {
    pub router: DeviceId,
    pub net: Ipv6Net,
    pub via: Ipv6Addr,
    pub iface: String,
    /// RIPng's metric when it was lost.
    pub metric: u8,
    /// No worse route to the prefix is taken before this time (µs).
    pub holddown_until: u64,
    /// Whether the hold-down has run out (and been acted on).
    pub hold_done: bool,
    /// Gone from the table at this time.
    pub flush_at: u64,
}

/// A neighbour no longer heard, kept until the invalid timer runs out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held6 {
    pub router: DeviceId,
    pub iface: String,
    /// The neighbour's link-local address.
    pub address: Ipv6Addr,
    pub peer: DeviceId,
    pub until: u64,
}

/// A router's timers in µs: update, invalid, hold-down, flush.
pub fn timers_us(d: &Device) -> (u64, u64, u64, u64) {
    let (u, i, h, f) = d.rip6.as_ref().map_or((30, 180, 180, 300), |r| r.timers());
    let us = |s: u32| u64::from(s.max(1)) * 1_000_000;
    (us(u), us(i), us(h), us(f))
}

/// RIPng's view of the lab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rip6State {
    /// Routes possibly down per router.
    pub down: BTreeMap<DeviceId, Vec<Rip6Down>>,
    /// Neighbours no longer heard but still within the invalid time.
    pub held: Vec<Held6>,
    pub routes: BTreeMap<DeviceId, Vec<Rip6Route>>,
    /// Neighbours per router: (their device, their link-local address, our interface).
    pub neighbors: BTreeMap<DeviceId, Vec<(DeviceId, Ipv6Addr, String)>>,
    /// Interfaces running RIPng per router, with their global prefixes.
    pub interfaces: BTreeMap<DeviceId, Vec<(String, Vec<Ipv6Net>)>>,
}

/// Whether a router runs RIPng: configured, forwarding IPv6.
pub fn running(d: &Device) -> bool {
    d.rip6.is_some() && routes6(d)
}

fn ifaces(t: &Topology, d: &Device) -> Vec<L3If6> {
    let Some(name) = d.rip6.as_ref().map(|r| r.name.clone()) else { return Vec::new() };
    up_ifaces6(t, d).into_iter().filter(|i| conf6(d, i).and_then(|c| c.rip.as_ref()) == Some(&name)).collect()
}

/// One entry while computing: hops, and (our interface, next hop, neighbour) unless connected.
type Entry = (u8, Option<(String, Ipv6Addr, DeviceId)>);

/// RIPng converged over the lab as it is now, every neighbour heard.
pub fn compute(t: &Topology) -> Rip6State {
    compute_with(t, &[])
}

/// [`compute`] with neighbours that are no longer heard but still within their invalid time.
pub fn compute_with(t: &Topology, held: &[Held6]) -> Rip6State {
    let mut st = Rip6State::default();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| running(d)).collect();
    let mut ifs: BTreeMap<DeviceId, Vec<L3If6>> = BTreeMap::new();
    for d in &routers {
        let list = ifaces(t, d);
        st.interfaces.insert(
            d.id,
            list.iter()
                .map(|i| (i.name.clone(), i.addresses.iter().map(|a| a.network()).filter(|n| !Ipv6Net::is_link_local(n.addr)).collect()))
                .collect(),
        );
        ifs.insert(d.id, list);
    }
    let mut adj: BTreeMap<DeviceId, Vec<(String, Ipv6Addr, DeviceId)>> = BTreeMap::new();
    for a in &routers {
        for ia in ifs.get(&a.id).into_iter().flatten() {
            let r = crate::ospf6::reach(t, a, ia);
            for b in routers.iter().filter(|b| b.id != a.id) {
                for ib in ifs.get(&b.id).into_iter().flatten() {
                    let same_vlan = ib.vlan == ia.vlan || ia.port == SVI_PORT || ib.port == SVI_PORT;
                    if same_vlan && crate::ospf6::reaches(t, &r, b, ib) {
                        adj.entry(a.id).or_default().push((ia.name.clone(), ib.link_local, b.id));
                        st.neighbors.entry(a.id).or_default().push((b.id, ib.link_local, ia.name.clone()));
                    }
                }
            }
        }
    }
    // Neighbours still within their invalid time: as if heard.
    for h in held {
        let Some(ia) = ifs.get(&h.router).into_iter().flatten().find(|i| i.name == h.iface) else { continue };
        if adj.get(&h.router).is_some_and(|l| l.iter().any(|x| x.0 == h.iface && x.1 == h.address)) {
            continue;
        }
        adj.entry(h.router).or_default().push((ia.name.clone(), h.address, h.peer));
        st.neighbors.entry(h.router).or_default().push((h.peer, h.address, h.iface.clone()));
    }
    st.held = held.to_vec();
    let mut tables: BTreeMap<DeviceId, BTreeMap<Ipv6Net, Entry>> = BTreeMap::new();
    for (id, list) in &st.interfaces {
        let mut tbl = BTreeMap::new();
        for (_, nets) in list {
            for n in nets {
                tbl.insert(*n, (0, None));
            }
        }
        tables.insert(*id, tbl);
    }
    for _ in 0..=INFINITY {
        let snapshot = tables.clone();
        let mut changed = false;
        for d in &routers {
            let Some(tbl) = tables.get_mut(&d.id) else { continue };
            for (ifc, addr, from) in adj.get(&d.id).into_iter().flatten() {
                for (net, (hops, learned)) in snapshot.get(from).into_iter().flatten() {
                    if learned.as_ref().is_some_and(|(_, _, back)| *back == d.id) {
                        continue;
                    }
                    let h = hops.saturating_add(1);
                    if h >= INFINITY {
                        continue;
                    }
                    let better = match tbl.get(net) {
                        None => true,
                        Some((old, None)) => h < *old,
                        Some((old, Some((_, oa, _)))) => h < *old || (h == *old && *addr < *oa),
                    };
                    if better {
                        tbl.insert(*net, (h, Some((ifc.clone(), *addr, *from))));
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
        let routes = tbl
            .into_iter()
            .filter_map(|(net, (hops, via))| via.map(|(iface, addr, _)| Rip6Route { net, via: addr, iface, metric: hops.saturating_add(1) }))
            .collect();
        st.routes.insert(id, routes);
    }
    st
}

/// A fingerprint of everything RIPng depends on.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    crate::eigrp6::fingerprint(t).hash(&mut h);
    for d in &t.devices {
        format!("{:?}", d.rip6).hash(&mut h);
    }
    h.finish()
}

/// The entries a router puts in a response out of one interface: its other connected prefixes
/// (metric 1), its routes (16 for those learned through this interface: poisoned reverse) and
/// the routes possibly down (16).
pub fn update_entries(st: &Rip6State, me: DeviceId, out: &str) -> Vec<RipngEntry> {
    let ifs = st.interfaces.get(&me).cloned().unwrap_or_default();
    let mut entries: Vec<RipngEntry> =
        ifs.iter().filter(|(n, _)| n != out).flat_map(|(_, nets)| nets.iter().map(|n| RipngEntry { net: *n, tag: 0, metric: 1 })).collect();
    for r in st.routes.get(&me).into_iter().flatten() {
        entries.push(RipngEntry { net: r.net, tag: 0, metric: if r.iface == out { INFINITY } else { r.metric } });
    }
    for x in st.down.get(&me).into_iter().flatten() {
        if !entries.iter().any(|e| e.net == x.net) {
            entries.push(RipngEntry { net: x.net, tag: 0, metric: INFINITY });
        }
    }
    entries
}

impl Sim {
    /// Brings RIPng's state up to date at the current simulated time (see the module notes),
    /// as `rip_refresh` does for RIPv2.
    pub(crate) fn rip6_refresh(&mut self, t: &Topology) {
        let now = self.now;
        if !t.devices.iter().any(|d| d.rip6.is_some()) {
            self.rip6 = Some(Rip6State::default());
            self.rip6_held.clear();
            self.rip6_down.clear();
            return;
        }
        let old = self.rip6.take();
        self.rip6_held.retain(|h| h.until > now);
        let raw = compute(t);
        let heard =
            |st: &Rip6State, r: DeviceId, iface: &str, a: Ipv6Addr| st.neighbors.get(&r).is_some_and(|l| l.iter().any(|n| n.2 == iface && n.1 == a));
        let iface_up = |st: &Rip6State, r: DeviceId, iface: &str| st.interfaces.get(&r).is_some_and(|l| l.iter().any(|i| i.0 == iface));
        self.rip6_held.retain(|h| !heard(&raw, h.router, &h.iface, h.address) && iface_up(&raw, h.router, &h.iface));
        if let Some(old) = &old {
            for (me, list) in &old.neighbors {
                let Some(d) = t.device(*me) else { continue };
                for (peer, addr, iface) in list {
                    if heard(&raw, *me, iface, *addr) || !iface_up(&raw, *me, iface) {
                        continue;
                    }
                    if self.rip6_held.iter().any(|h| h.router == *me && h.iface == *iface && h.address == *addr)
                        || old.held.iter().any(|h| h.router == *me && h.iface == *iface && h.address == *addr && h.until <= now)
                    {
                        continue;
                    }
                    // Silent: its routes stay until the invalid time after its last response.
                    let (update, invalid, ..) = timers_us(d);
                    let until = (now - now % update).saturating_add(invalid);
                    if until > now && self.rip6_held.len() < MAX_HELD {
                        self.rip6_held.push(Held6 { router: *me, iface: iface.clone(), address: *addr, peer: *peer, until });
                    }
                }
            }
        }
        let mut new = if self.rip6_held.is_empty() { raw } else { compute_with(t, &self.rip6_held) };
        // Routes lost since the last look are possibly down.
        self.rip6_down.retain(|x| x.flush_at > now && t.device(x.router).is_some_and(|d| d.rip6.is_some()));
        if let Some(old) = &old {
            for (me, routes) in &old.routes {
                let Some(d) = t.device(*me) else { continue };
                let (_, invalid, holddown, flush) = timers_us(d);
                for r in routes {
                    let still = new.routes.get(me).is_some_and(|l| l.iter().any(|x| x.net == r.net));
                    if still || self.rip6_down.iter().any(|x| x.router == *me && x.net == r.net) || self.rip6_down.len() >= MAX_HELD {
                        continue;
                    }
                    self.rip6_down.push(Rip6Down {
                        router: *me,
                        net: r.net,
                        via: r.via,
                        iface: r.iface.clone(),
                        metric: r.metric,
                        holddown_until: now.saturating_add(holddown),
                        hold_done: false,
                        flush_at: now.saturating_add(flush.saturating_sub(invalid).max(1)),
                    });
                }
            }
        }
        // Hold-down: no worse route to a prefix that went down until it runs out.
        for (me, routes) in new.routes.iter_mut() {
            routes.retain(|r| !self.rip6_down.iter().any(|x| x.router == *me && x.net == r.net && x.holddown_until > now && r.metric > x.metric));
        }
        // A route that came back ends its possibly-down entry.
        self.rip6_down.retain(|x| !new.routes.get(&x.router).is_some_and(|l| l.iter().any(|r| r.net == x.net)));
        for x in &mut self.rip6_down {
            x.hold_done = x.holddown_until <= now;
        }
        for x in &self.rip6_down {
            new.down.entry(x.router).or_default().push(x.clone());
        }
        new.held = self.rip6_held.clone();
        if let Some(old) = &old {
            self.rip6_triggered(t, old, &new);
        }
        self.rip6 = Some(new);
    }

    /// When RIPng's next timer runs out: a silent neighbour's invalid time, a hold-down or a flush.
    pub(crate) fn rip6_next_expiry(&self) -> Option<u64> {
        let held = self.rip6_held.iter().map(|h| h.until);
        let down = self.rip6_down.iter().map(|x| if x.hold_done { x.flush_at } else { x.holddown_until.min(x.flush_at) });
        held.chain(down).min()
    }

    /// Triggered responses (RFC 2080 §2.5.1): each router whose routes changed sends, a second
    /// later, the changed entries out of each RIPng interface with neighbours.
    fn rip6_triggered(&mut self, t: &Topology, old: &Rip6State, new: &Rip6State) {
        let at = self.now.saturating_add(TRIGGER_DELAY_US);
        let mut out: Vec<(DeviceId, String, Vec<RipngEntry>)> = Vec::new();
        for (me, ifs) in &new.interfaces {
            // A router just starting sends its whole table with the regular responses.
            let known_before = old.interfaces.contains_key(me);
            for (name, _) in ifs {
                if !new.neighbors.get(me).is_some_and(|l| l.iter().any(|n| n.2 == *name)) {
                    continue;
                }
                let was: BTreeMap<Ipv6Net, u8> = update_entries(old, *me, name).into_iter().map(|e| (e.net, e.metric)).collect();
                let now_entries = update_entries(new, *me, name);
                let mut changed: Vec<RipngEntry> = now_entries.iter().filter(|e| was.get(&e.net) != Some(&e.metric)).copied().collect();
                for net in was.keys().filter(|k| !now_entries.iter().any(|e| e.net == **k)) {
                    changed.push(RipngEntry { net: *net, tag: 0, metric: INFINITY });
                }
                if !changed.is_empty() && known_before {
                    out.push((*me, name.clone(), changed));
                }
            }
        }
        for (me, name, entries) in out {
            let Some(d) = t.device(me) else { continue };
            self.ripng_send(t, d, &name, &entries, at);
        }
    }

    /// Regular RIPng responses at each router's update timer: every RIPng interface's other
    /// prefixes and routes, from its link-local address to `FF02::9`.
    pub(crate) fn send_ripng(&mut self, t: &Topology, at: u64) {
        if !t.devices.iter().any(|d| d.rip6.is_some()) {
            return;
        }
        let st = self.rip6_state(t);
        for (id, ifs) in &st.interfaces {
            let Some(d) = t.device(*id) else { continue };
            let (update, ..) = timers_us(d);
            if !at.is_multiple_of(update.max(1_000_000)) {
                continue;
            }
            for (name, _) in ifs {
                let entries = update_entries(&st, *id, name);
                self.ripng_send(t, d, name, &entries, at);
            }
        }
    }

    /// Sends RIPng responses with `entries` out of an interface (as many as they take).
    pub(crate) fn ripng_send(&mut self, t: &Topology, d: &Device, iface: &str, entries: &[RipngEntry], at: u64) {
        use netlabcraft_wire::ipv6::{ETHERTYPE_IPV6, Ipv6, multicast_mac};
        use netlabcraft_wire::{Frame, PROTO_UDP};
        let up = up_ifaces6(t, d);
        let Some(ifc) = up.iter().find(|i| i.name == iface) else { return };
        if crate::loopback::is_loopback(ifc.port) {
            return;
        }
        for chunk in entries.chunks(ripng::MAX_ENTRIES).take(16) {
            let payload = netlabcraft_wire::dhcp6::udp6_encode(ifc.link_local, RIPNG_ROUTERS, RIPNG_PORT, RIPNG_PORT, &ripng::encode(chunk));
            let ip = Ipv6 { hop_limit: 255, next_header: PROTO_UDP, src: ifc.link_local, dst: RIPNG_ROUTERS, payload };
            let frame = Frame { dst: multicast_mac(RIPNG_ROUTERS), src: ifc.mac, vlan: ifc.vlan, ethertype: ETHERTYPE_IPV6, payload: ip.encode() };
            self.queue_frame(t, d, ifc.port, frame, at);
        }
    }
}
