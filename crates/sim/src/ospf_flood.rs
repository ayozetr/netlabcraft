//! OSPF's neighbour state machine and flooding on the wire (RFC 2328 §10 and §13).
//!
//! The routes and the database a router ends up with are still worked out from the lab (see
//! [`crate::ospf`] and [`crate::ospf_lsdb`]), but the packets are real: a router that hears a hello
//! creates a neighbour in Init, the hellos list each other and the neighbour goes to 2-Way, a
//! point-to-point neighbour (or one that is the DR or BDR of a broadcast segment, or this router's
//! own DR or BDR) goes on to ExStart, the two negotiate who is master, describe their databases
//! (DBD), request what they lack (LSR), receive it (LSU) and acknowledge it (LSAck) before the
//! neighbour is Full. Every router keeps a copy of each LSA it has learned this way (`held`), a
//! per-neighbour list of the updates not yet acknowledged, and sends them again every
//! `ip ospf retransmit-interval` seconds (5 by default) until they are. A router that originates
//! a new instance of an LSA floods it to its adjacent neighbours, and each router that installs it
//! floods it on (RFC 2328 §13.3). Copies age on the simulated clock: the originator refreshes its
//! LSAs every 1800 s and a copy nobody refreshed leaves the router at 3600 s (MaxAge).
//!
//! Hellos are built when they are due, not when the control traffic is scheduled, so they list the
//! neighbours as they are at that moment (see `docs/pitfalls.md`).
//!
//! A neighbour that has had no packet yet (the simulator has not run since it appeared) is shown by
//! the state the lab converges to (Full), so a lab that is configured and looked at without time
//! passing still reads as before.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::Ipv4;
use netlabcraft_wire::control::{self, OspfAuthSend, OspfHello};
use netlabcraft_wire::ospfdb::{
    DBD_INIT, DBD_MASTER, DBD_MORE, LsaHeader, MAX_AGE, OspfDb, RawLsa, RouterLink, external_body, network_body, router_body, seq_newer, summary_body,
};

use crate::ospf::{self, AreaKind, OspfState};
use crate::ospf_lsdb::{self, LinkKind, Lsa, LsaKind};
use crate::{DebugKind, L3If, Sim, l3_ifaces};

/// AllDRouters, where a router that is neither DR nor BDR sends its updates on a broadcast segment.
const ALL_DR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 6);
/// Most LSA headers in one database description.
const DBD_BATCH: usize = 40;
/// Most requests in one link-state request.
const REQ_BATCH: usize = 50;
/// Most LSAs in one link-state update.
const LSU_BATCH: usize = 8;
/// Most LSAs one router keeps, and most requests one neighbour holds.
const MAX_HELD: usize = 16_384;
const MAX_REQUESTS: usize = 16_384;
/// The originator sends an LSA again this often (LSRefreshTime).
const LS_REFRESH_US: u64 = 1_800_000_000;

/// A neighbour's state (RFC 2328 §10.1). A neighbour that is gone is Down and has no entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NbrState {
    Init,
    TwoWay,
    ExStart,
    Exchange,
    Loading,
    Full,
}

impl NbrState {
    /// The name `debug ip ospf adj` uses.
    pub fn name(self) -> &'static str {
        match self {
            NbrState::Init => "Init",
            NbrState::TwoWay => "2-Way",
            NbrState::ExStart => "ExStart",
            NbrState::Exchange => "Exchange",
            NbrState::Loading => "Loading",
            NbrState::Full => "Full",
        }
    }

    /// The word `show ip ospf neighbor` prints.
    pub fn word(self) -> &'static str {
        match self {
            NbrState::Init => "INIT",
            NbrState::TwoWay => "2WAY",
            NbrState::ExStart => "EXSTART",
            NbrState::Exchange => "EXCHANGE",
            NbrState::Loading => "LOADING",
            NbrState::Full => "FULL",
        }
    }
}

/// What names an LSA in a packet: its type, link-state ID and advertising router.
type Key = (u8, Ipv4Addr, Ipv4Addr);
/// What names an LSA in a router's copy of the database: its area as well (none for AS-external).
pub type HeldKey = (Option<u32>, u8, Ipv4Addr, Ipv4Addr);
/// A neighbour: the router, its interface and the neighbour's router ID.
type NKey = (DeviceId, String, Ipv4Addr);

/// The area an LSA of this type floods in, when it arrives on an interface of `area`.
fn lsa_area(kind: u8, area: u32) -> Option<u32> {
    if kind == 5 { None } else { Some(area) }
}

/// Whether an LSA goes out of (or comes in on) an interface of `area`: area-scoped LSAs stay in
/// their area, AS-external ones go everywhere but into stub areas and NSSAs.
fn in_scope(d: &Device, area: u32, key: &HeldKey) -> bool {
    match key.0 {
        Some(a) => a == area,
        None => ospf::area_kind(d, area) == AreaKind::Normal,
    }
}

/// An LSA's age at `now`, given its age when it was installed at `at`.
fn age_at(age: u16, at: u64, now: u64) -> u16 {
    let secs = now.saturating_sub(at) / 1_000_000;
    u16::try_from(u64::from(age).saturating_add(secs).min(u64::from(MAX_AGE))).unwrap_or(MAX_AGE)
}

/// A router's copy of one LSA.
#[derive(Debug, Clone)]
pub struct Held {
    /// The LSA as installed, with the age it had then.
    pub lsa: RawLsa,
    /// When it was installed (simulated microseconds).
    pub at: u64,
}

impl Held {
    pub fn age(&self, now: u64) -> u16 {
        age_at(self.lsa.header.age, self.at, now)
    }
}

/// One neighbour's runtime state.
#[derive(Debug, Clone)]
pub struct Nbr {
    pub state: NbrState,
    pub addr: Ipv4Addr,
    mac: MacAddr,
    pub area: u32,
    /// When it entered its present state.
    pub since: u64,
    /// This router is the master of the database exchange.
    master: bool,
    dd_seq: u32,
    /// LSA headers still to be described to the neighbour.
    summary: VecDeque<LsaHeader>,
    /// The M bit of the last description sent, and of the last one received.
    sent_more: bool,
    their_more: bool,
    last_dbd: Option<OspfDb>,
    /// LSAs the neighbour has that this router lacks.
    request: VecDeque<Key>,
    /// Updates sent and not acknowledged: the instance sent and when it was sent.
    retrans: BTreeMap<Key, (RawLsa, u64)>,
    /// When something is next sent again.
    due: Option<u64>,
    /// How many times a packet was sent again.
    pub rxmt_sent: u32,
}

impl Nbr {
    fn new(addr: Ipv4Addr, mac: MacAddr, area: u32, now: u64) -> Nbr {
        Nbr {
            state: NbrState::Init,
            addr,
            mac,
            area,
            since: now,
            master: false,
            dd_seq: 0,
            summary: VecDeque::new(),
            sent_more: false,
            their_more: false,
            last_dbd: None,
            request: VecDeque::new(),
            retrans: BTreeMap::new(),
            due: None,
            rxmt_sent: 0,
        }
    }

    /// Whether this router waits for an answer to something it sent.
    fn outstanding(&self) -> bool {
        self.state == NbrState::ExStart
            || (self.state == NbrState::Exchange && self.master)
            || (self.state == NbrState::Loading && !self.request.is_empty())
            || !self.retrans.is_empty()
    }

    /// The sizes of the neighbour's request list and retransmission list (for the views).
    pub fn pending(&self) -> (usize, usize) {
        (self.request.len(), self.retrans.len())
    }
}

/// Everything the flooding keeps, for the whole lab.
#[derive(Debug, Clone, Default)]
pub struct Runtime {
    pub nbrs: BTreeMap<NKey, Nbr>,
    /// Each router's copy of the database.
    pub held: BTreeMap<DeviceId, BTreeMap<HeldKey, Held>>,
    /// Hellos due: (time, router, interface). Built when they are due.
    hello_due: BTreeSet<(u64, DeviceId, String)>,
    /// When a hello was last sent at once for an adjacency that had no record.
    trigger: BTreeMap<(DeviceId, String), u64>,
    /// A fault for tests: how many link-state updates a router drops on arrival.
    lose: BTreeMap<DeviceId, u32>,
    /// The earliest time a copy is due to be refreshed or aged out (a lower bound).
    hold_due: Option<u64>,
}

/// Where a packet goes.
#[derive(Debug, Clone, Copy)]
enum Dest {
    Unicast(Ipv4Addr, MacAddr),
    AllSpf,
    AllDr,
}

/// What a handler needs to know about a router's interface.
struct Cx<'a> {
    t: &'a Topology,
    st: &'a OspfState,
    d: &'a Device,
    ifc: L3If,
    rid: Ipv4Addr,
    area: u32,
    seg: Option<ospf::Segment>,
    /// The retransmit interval, in microseconds.
    rxmt: u64,
    /// The transmit delay added to the age of an LSA sent here, in seconds.
    delay: u16,
}

fn context<'a>(t: &'a Topology, st: &'a OspfState, dev: DeviceId, iface: &str) -> Option<Cx<'a>> {
    let d = t.device(dev)?;
    let rid = st.router_ids.get(&dev).copied()?;
    let ifc = l3_ifaces(d).into_iter().find(|i| i.name == iface)?;
    let seg = st.segments.get(&(dev, iface.to_string())).copied();
    let conf = ospf::if_conf(d, iface);
    Some(Cx {
        t,
        st,
        d,
        ifc,
        rid,
        area: seg.map_or(0, |s| s.area),
        seg,
        rxmt: u64::from(conf.retransmit.unwrap_or(5).max(1)) * 1_000_000,
        delay: conf.transmit_delay.unwrap_or(1),
    })
}

fn min_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// The wire form of an LSA of the computed database, with the sequence number it has and `age`.
pub fn to_raw(l: &Lsa, age: u16) -> RawLsa {
    let mask = netlabcraft_core::prefix_to_mask(u8::try_from(l.count).unwrap_or(0));
    let none = Ipv4Addr::UNSPECIFIED;
    let (kind, body) = match l.kind {
        LsaKind::Router => {
            let links: Vec<RouterLink> = l
                .links
                .iter()
                .map(|k| RouterLink {
                    kind: match k.kind {
                        LinkKind::PointToPoint => 1,
                        LinkKind::Transit => 2,
                        LinkKind::Stub => 3,
                    },
                    id: k.id,
                    data: k.data,
                    metric: u16::try_from(k.metric).unwrap_or(u16::MAX),
                })
                .collect();
            (1, router_body(if l.nt { 0x10 } else { 0 }, &links))
        }
        LsaKind::Network => (2, network_body(mask, &l.attached)),
        LsaKind::Summary => (3, summary_body(mask, l.metric)),
        LsaKind::AsbrSummary => (4, summary_body(none, l.metric)),
        LsaKind::External => (5, external_body(mask, !l.type1, l.metric, l.forward.unwrap_or(none), l.tag)),
        LsaKind::Nssa => (7, external_body(mask, !l.type1, l.metric, l.forward.unwrap_or(none), l.tag)),
    };
    RawLsa::new(age, kind, l.id, l.adv, l.seq, body)
}

fn kind_word(kind: u8) -> &'static str {
    match kind {
        1 => "router",
        2 => "network",
        3 => "summary",
        4 => "ASBR-summary",
        5 => "external",
        7 => "NSSA-external",
        _ => "unknown",
    }
}

fn describe(h: &LsaHeader) -> String {
    format!("{} LSA {} advertised by {} (sequence 0x{:08x}, age {})", kind_word(h.kind), h.id, h.advertising, h.seq, h.age)
}

impl Sim {
    /// Runs `f` with OSPF's computed state, brought up to date first. The state is taken out of the
    /// simulator while `f` runs, so `f` must not ask for it again.
    pub(crate) fn with_ospf<R>(&mut self, t: &Topology, f: impl FnOnce(&mut Sim, &OspfState) -> R) -> Option<R> {
        let fp = ospf::fingerprint(t);
        if self.ospf.is_none() || self.ospf_for != fp {
            self.ospf_for = fp;
            self.ospf = Some(self.ospf_compute(t));
            self.ospf_restamp(t);
        }
        let st = self.ospf.take()?;
        let r = f(self, &st);
        self.ospf = Some(st);
        Some(r)
    }

    /// The neighbour's state, when packets have been exchanged with it.
    pub fn ospf_neighbor_state(&self, d: DeviceId, iface: &str, rid: Ipv4Addr) -> Option<NbrState> {
        self.ospf_rt.nbrs.get(&(d, iface.to_string(), rid)).map(|n| n.state)
    }

    /// The neighbour's runtime record (its lists and counters), when there is one.
    pub fn ospf_neighbor(&self, d: DeviceId, iface: &str, rid: Ipv4Addr) -> Option<&Nbr> {
        self.ospf_rt.nbrs.get(&(d, iface.to_string(), rid))
    }

    /// The LSAs a router has learned (and originated) by flooding: key, sequence number and age.
    pub fn ospf_held(&self, d: DeviceId) -> Vec<(HeldKey, u32, u16)> {
        let now = self.now;
        self.ospf_rt.held.get(&d).map(|m| m.iter().map(|(k, h)| (*k, h.lsa.header.seq, h.age(now))).collect()).unwrap_or_default()
    }

    /// Makes a router drop the next `n` link-state updates it receives, as if the network lost
    /// them (for tests of the retransmission).
    pub fn ospf_lose_updates(&mut self, d: DeviceId, n: u32) {
        self.ospf_rt.lose.insert(d, n);
    }

    /// Sends a packet from this router's interface.
    fn ospf_tx(&mut self, cx: &Cx, to: Dest, pkt: &OspfDb) {
        let clock = self.clock(cx.d.id).0;
        let at = self.now;
        let sign = ospf::auth_sends(cx.d, &cx.ifc, at, clock).into_iter().next().unwrap_or(OspfAuthSend::Null);
        let payload = control::ospf_authenticate(pkt.encode(cx.rid, cx.area), &sign);
        let (dst, mac) = match to {
            Dest::Unicast(ip, mac) => (ip, mac),
            Dest::AllSpf => (control::OSPF_ALL_ROUTERS, control::ipv4_multicast_mac(control::OSPF_ALL_ROUTERS)),
            Dest::AllDr => (ALL_DR, control::ipv4_multicast_mac(ALL_DR)),
        };
        let ip = Ipv4 { id: 0, ttl: 1, protocol: control::PROTO_OSPF, src: cx.ifc.net.addr, dst, payload };
        self.queue_l3(cx.t, cx.d, &cx.ifc, cx.ifc.mac, mac, ip.encode(), at);
    }

    fn nbr_dest(&self, key: &NKey) -> Option<Dest> {
        self.ospf_rt.nbrs.get(key).map(|n| Dest::Unicast(n.addr, n.mac))
    }

    /// Changes a neighbour's state and reports it under `debug ip ospf adj`.
    fn ospf_set_state(&mut self, key: &NKey, new: NbrState, why: &str) {
        let now = self.now;
        let Some(n) = self.ospf_rt.nbrs.get_mut(key) else { return };
        let old = n.state;
        if old == new {
            return;
        }
        n.state = new;
        n.since = now;
        let text = format!("OSPF adjacency: neighbour {} on {}: {} -> {}: {why}", key.2, key.1, old.name(), new.name());
        self.debug_raw(key.0, DebugKind::OspfAdj, &text);
    }

    /// Arms the retransmission timer of a neighbour: when something is outstanding and no timer
    /// runs, it starts; when nothing is, it stops. `restart` starts the interval over.
    fn ospf_arm(&mut self, key: &NKey, rxmt: u64, restart: bool) {
        let now = self.now;
        if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
            if !n.outstanding() {
                n.due = None;
            } else if restart || n.due.is_none() {
                n.due = Some(now.saturating_add(rxmt));
            }
        }
    }

    // ---- hellos ----

    /// Queues the hello an interface is due to send at `at`.
    pub(crate) fn ospf_hello_later(&mut self, d: DeviceId, iface: &str, at: u64) {
        self.ospf_rt.hello_due.insert((at, d, iface.to_string()));
    }

    /// Sends a hello out of an interface, listing the neighbours heard on it now.
    pub(crate) fn ospf_hello(&mut self, t: &Topology, st: &OspfState, d: &Device, name: &str, at: u64) {
        let Some(cx) = context(t, st, d.id, name) else { return };
        let heard: Vec<Ipv4Addr> =
            self.ospf_rt.nbrs.iter().filter(|(k, n)| k.0 == d.id && k.1 == name && n.state >= NbrState::Init).map(|(k, _)| k.2).collect();
        // DR and BDR are only announced once there is someone to elect with.
        let elected = |a: Option<Ipv4Addr>| a.filter(|_| !heard.is_empty()).unwrap_or(Ipv4Addr::UNSPECIFIED);
        let (hello_secs, dead) = ospf::timers(d, name);
        let hello = OspfHello {
            router_id: cx.rid,
            area: cx.area,
            mask: cx.ifc.net.mask(),
            hello_secs,
            dead_secs: u32::from(dead),
            priority: cx.seg.map_or(1, |s| s.priority),
            dr: elected(cx.seg.and_then(|s| s.dr)),
            bdr: elected(cx.seg.and_then(|s| s.bdr)),
            neighbors: heard.clone(),
            options: ospf::hello_options(d, &cx.ifc),
        };
        // One copy per key while several message-digest keys are held (a key rollover).
        let now = self.clock(d.id).0;
        let encoded = hello.encode();
        for (n, sign) in (0u64..).zip(ospf::auth_sends(d, &cx.ifc, at, now)) {
            let payload = control::ospf_authenticate(encoded.clone(), &sign);
            let ip = Ipv4 { id: 0, ttl: 1, protocol: control::PROTO_OSPF, src: cx.ifc.net.addr, dst: control::OSPF_ALL_ROUTERS, payload };
            self.queue_l3(t, d, &cx.ifc, cx.ifc.mac, control::ipv4_multicast_mac(control::OSPF_ALL_ROUTERS), ip.encode(), at + n * 100);
        }
    }

    // ---- keeping the records up to date ----

    /// Called when the simulator starts running: neighbours that went away lose their record, and an
    /// adjacency with no record yet gets a hello at once, so it does not wait for the next tick.
    pub(crate) fn ospf_sync(&mut self, t: &Topology) {
        if self.ospf_rt.nbrs.is_empty() && self.ospf_rt.held.is_empty() && !t.devices.iter().any(|d| d.ospf.is_some()) {
            return;
        }
        self.with_ospf(t, |s, st| s.ospf_sync_in(t, st));
    }

    fn ospf_sync_in(&mut self, t: &Topology, st: &OspfState) {
        let now = self.now;
        let wanted: BTreeSet<NKey> = st.neighbors.iter().flat_map(|(id, ns)| ns.iter().map(move |n| (*id, n.iface.clone(), n.router_id))).collect();
        let gone: Vec<NKey> = self.ospf_rt.nbrs.keys().filter(|k| !wanted.contains(*k)).cloned().collect();
        for k in gone {
            let Some(n) = self.ospf_rt.nbrs.remove(&k) else { continue };
            let dead = t.device(k.0).map_or(40, |d| ospf::timers(d, &k.1).1);
            self.debug_raw(
                k.0,
                DebugKind::OspfAdj,
                &format!("OSPF adjacency: neighbour {} on {}: {} -> Down: no hello within {dead} s, or the link is gone", k.2, k.1, n.state.name()),
            );
            if self.debug_on(k.0, DebugKind::OspfEvents) {
                let routes = st.routes.get(&k.0).map_or(0, Vec::len);
                self.debug_raw(k.0, DebugKind::OspfEvents, &format!("OSPF event: neighbour {} on {} removed; its LSAs are no longer used", k.2, k.1));
                self.debug_raw(
                    k.0,
                    DebugKind::OspfEvents,
                    &format!("OSPF event: SPF run after the loss: {routes} OSPF route{}", if routes == 1 { "" } else { "s" }),
                );
            }
        }
        // A router that no longer runs OSPF keeps no database.
        self.ospf_rt.held.retain(|d, _| st.router_ids.contains_key(d));
        self.ospf_rt.trigger.retain(|k, _| st.router_ids.contains_key(&k.0));
        let need: BTreeSet<(DeviceId, String)> = wanted.iter().filter(|k| !self.ospf_rt.nbrs.contains_key(*k)).map(|k| (k.0, k.1.clone())).collect();
        for (dev, iface) in need {
            if self.ospf_rt.trigger.get(&(dev, iface.clone())).is_some_and(|last| now < last.saturating_add(1_000_000)) {
                continue;
            }
            self.ospf_rt.trigger.insert((dev, iface.clone()), now);
            if let Some(d) = t.device(dev) {
                self.ospf_hello(t, st, d, &iface, now);
            }
        }
        self.ospf_rt.hold_due = self.ospf_hold_due(&st.router_ids);
    }

    /// `clear ip ospf process`: every neighbour starts over and every router forgets what it learned.
    pub(crate) fn ospf_rt_clear(&mut self) {
        let hellos = std::mem::take(&mut self.ospf_rt.hello_due);
        self.ospf_rt = Runtime { hello_due: hellos, ..Runtime::default() };
    }

    // ---- receiving ----

    /// An OSPF packet arrived on `ifc` of router `d`.
    pub(crate) fn ospf_rx(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4) {
        let clock = self.clock(d.id).0;
        if ospf::auth_problem(d, ifc, &ip.payload, clock).is_some() {
            return;
        }
        let name = ifc.name.clone();
        self.with_ospf(t, |s, st| {
            if st.interfaces.get(&d.id).into_iter().flatten().any(|x| x.0 == name && x.3) {
                return;
            }
            let Some(cx) = context(t, st, d.id, &name) else { return };
            match ip.payload.get(1).copied() {
                Some(1) => {
                    let Ok(h) = OspfHello::decode(&ip.payload) else { return };
                    if ospf::hello_problem(d, ifc, &h).is_some() {
                        return;
                    }
                    s.rx_hello(&cx, ip.src, &h);
                }
                Some(2..=5) => {
                    let Ok((rid, db)) = OspfDb::decode(&ip.payload) else { return };
                    let key: NKey = (d.id, name.clone(), rid);
                    let Some(state) = s.ospf_rt.nbrs.get(&key).map(|n| n.state) else { return };
                    match db {
                        OspfDb::Dbd { flags, seq, headers } if state >= NbrState::ExStart => s.rx_dbd(&cx, &key, flags, seq, headers),
                        OspfDb::Request(list) if state >= NbrState::Exchange => s.rx_lsr(&cx, &key, list),
                        OspfDb::Update(lsas) if state >= NbrState::Exchange => s.rx_lsu(&cx, &key, lsas),
                        OspfDb::Ack(headers) if state >= NbrState::Exchange => s.rx_ack(&cx, &key, headers),
                        _ => {}
                    }
                }
                _ => {}
            }
        });
    }

    fn rx_hello(&mut self, cx: &Cx, src: Ipv4Addr, h: &OspfHello) {
        let Some(peer) = cx.st.neighbors.get(&cx.d.id).and_then(|v| v.iter().find(|n| n.iface == cx.ifc.name && n.router_id == h.router_id)) else {
            return;
        };
        let key: NKey = (cx.d.id, cx.ifc.name.clone(), h.router_id);
        let now = self.now;
        if !self.ospf_rt.nbrs.contains_key(&key) {
            let mac =
                cx.t.device(peer.device).and_then(|p| l3_ifaces(p).into_iter().find(|i| i.net.addr == peer.address)).map_or(MacAddr::ZERO, |i| i.mac);
            self.ospf_rt.nbrs.insert(key.clone(), Nbr::new(src, mac, cx.area, now));
            self.debug_raw(
                cx.d.id,
                DebugKind::OspfAdj,
                &format!("OSPF adjacency: neighbour {} on {}: Down -> Init: first hello heard", h.router_id, cx.ifc.name),
            );
            // Answer at once, listing the new neighbour, so it does not wait for the next hello.
            self.ospf_hello(cx.t, cx.st, cx.d, &cx.ifc.name, now);
        }
        let Some(state) = self.ospf_rt.nbrs.get(&key).map(|n| n.state) else { return };
        let lists_me = h.neighbors.contains(&cx.rid);
        if !lists_me {
            if state > NbrState::Init {
                if let Some(n) = self.ospf_rt.nbrs.get_mut(&key) {
                    n.summary.clear();
                    n.request.clear();
                    n.retrans.clear();
                    n.last_dbd = None;
                    n.due = None;
                }
                self.ospf_set_state(&key, NbrState::Init, "the neighbour's hello no longer lists this router");
            }
            return;
        }
        if state == NbrState::Init {
            self.ospf_set_state(&key, NbrState::TwoWay, "this router is listed in the neighbour's hello");
            if self.debug_on(cx.d.id, DebugKind::OspfEvents)
                && let Some(g) = cx.seg.filter(|g| g.dr.is_some())
            {
                let a = |x: Option<Ipv4Addr>| x.map_or("none".to_string(), |x| x.to_string());
                self.debug_raw(
                    cx.d.id,
                    DebugKind::OspfEvents,
                    &format!("OSPF event: neighbour change on {}: DR election gives DR {}, BDR {}", cx.ifc.name, a(g.dr), a(g.bdr)),
                );
            }
        }
        let state = self.ospf_rt.nbrs.get(&key).map_or(NbrState::Init, |n| n.state);
        let need = Self::needs_adjacency(cx, src);
        if state == NbrState::TwoWay && need {
            let why = if cx.seg.is_some_and(|s| s.dr.is_none()) {
                "point-to-point link, an adjacency is always needed"
            } else {
                "an adjacency is needed with the DR or BDR"
            };
            self.start_exstart(cx, &key, why);
        } else if state > NbrState::TwoWay && !need {
            self.ospf_set_state(&key, NbrState::TwoWay, "neither router is DR or BDR any more");
        }
    }

    /// Whether this router forms an adjacency with the neighbour at `addr`: always on a
    /// point-to-point link, on a broadcast segment only when one of the two is the DR or BDR.
    fn needs_adjacency(cx: &Cx, addr: Ipv4Addr) -> bool {
        let Some(seg) = cx.seg else { return true };
        if seg.dr.is_none() {
            return true;
        }
        let mine = cx.ifc.net.addr;
        [seg.dr, seg.bdr].into_iter().flatten().any(|a| a == mine || a == addr)
    }

    fn start_exstart(&mut self, cx: &Cx, key: &NKey, why: &str) {
        let seq = 0x0000_1000 + (u32::from(cx.rid) & 0xff);
        let dbd = OspfDb::Dbd { flags: DBD_INIT | DBD_MORE | DBD_MASTER, seq, headers: Vec::new() };
        {
            let Some(n) = self.ospf_rt.nbrs.get_mut(key) else { return };
            n.master = true;
            n.dd_seq = seq;
            n.summary.clear();
            n.request.clear();
            n.sent_more = true;
            n.their_more = true;
            n.last_dbd = Some(dbd.clone());
        }
        self.ospf_set_state(key, NbrState::ExStart, why);
        if let Some(to) = self.nbr_dest(key) {
            self.ospf_tx(cx, to, &dbd);
        }
        self.ospf_arm(key, cx.rxmt, true);
    }

    /// The headers of every LSA this router holds that the neighbour's area can see.
    fn build_summary(&mut self, cx: &Cx, key: &NKey) {
        let now = self.now;
        let list: VecDeque<LsaHeader> = self
            .ospf_rt
            .held
            .get(&cx.d.id)
            .map(|m| {
                m.iter()
                    .filter(|(k, h)| in_scope(cx.d, cx.area, k) && h.age(now) < MAX_AGE)
                    .map(|(_, h)| {
                        let mut header = h.lsa.header;
                        header.age = h.age(now);
                        header
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
            n.summary = list;
        }
    }

    /// Adds to the request list every header that names an LSA this router lacks or has an older
    /// instance of.
    fn want_headers(&mut self, cx: &Cx, key: &NKey, headers: &[LsaHeader]) {
        let mut add: Vec<Key> = Vec::new();
        {
            let held = self.ospf_rt.held.get(&cx.d.id);
            for h in headers {
                if !matches!(h.kind, 1..=5 | 7) || h.advertising == cx.rid {
                    continue;
                }
                let hk: HeldKey = (lsa_area(h.kind, cx.area), h.kind, h.id, h.advertising);
                if !in_scope(cx.d, cx.area, &hk) {
                    continue;
                }
                if held.and_then(|m| m.get(&hk)).is_none_or(|x| seq_newer(h.seq, x.lsa.header.seq)) {
                    add.push((h.kind, h.id, h.advertising));
                }
            }
        }
        if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
            for k in add {
                if n.request.len() < MAX_REQUESTS && !n.request.contains(&k) {
                    n.request.push_back(k);
                }
            }
        }
    }

    /// Sends the next database description: the master moves its sequence number on first.
    fn send_dbd(&mut self, cx: &Cx, key: &NKey, advance: bool) {
        let dbd = {
            let Some(n) = self.ospf_rt.nbrs.get_mut(key) else { return };
            if advance {
                n.dd_seq = n.dd_seq.wrapping_add(1);
            }
            let take = n.summary.len().min(DBD_BATCH);
            let headers: Vec<LsaHeader> = n.summary.drain(..take).collect();
            let more = !n.summary.is_empty();
            let flags = (if n.master { DBD_MASTER } else { 0 }) | (if more { DBD_MORE } else { 0 });
            n.sent_more = more;
            let dbd = OspfDb::Dbd { flags, seq: n.dd_seq, headers };
            n.last_dbd = Some(dbd.clone());
            dbd
        };
        if let Some(to) = self.nbr_dest(key) {
            self.ospf_tx(cx, to, &dbd);
        }
    }

    fn resend_dbd(&mut self, cx: &Cx, key: &NKey) {
        let last = self.ospf_rt.nbrs.get(key).and_then(|n| n.last_dbd.clone());
        if let (Some(dbd), Some(to)) = (last, self.nbr_dest(key)) {
            self.ospf_tx(cx, to, &dbd);
        }
    }

    fn rx_dbd(&mut self, cx: &Cx, key: &NKey, flags: u8, seq: u32, headers: Vec<LsaHeader>) {
        let Some((state, master, dd_seq)) = self.ospf_rt.nbrs.get(key).map(|n| (n.state, n.master, n.dd_seq)) else { return };
        let (init, more, ms) = (flags & DBD_INIT != 0, flags & DBD_MORE != 0, flags & DBD_MASTER != 0);
        let their = key.2;
        match state {
            NbrState::ExStart => {
                if init && more && ms && headers.is_empty() && their > cx.rid {
                    // The neighbour has the higher router ID: it is the master, this router follows its numbers.
                    if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
                        n.master = false;
                        n.dd_seq = seq;
                        n.their_more = true;
                    }
                    self.build_summary(cx, key);
                    self.ospf_set_state(key, NbrState::Exchange, "master is the neighbour (higher router ID), database descriptions follow");
                    self.send_dbd(cx, key, false);
                } else if !init && !ms && their < cx.rid && seq == dd_seq {
                    self.build_summary(cx, key);
                    self.ospf_set_state(key, NbrState::Exchange, "master is this router (higher router ID), database descriptions follow");
                    self.want_headers(cx, key, &headers);
                    if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
                        n.their_more = more;
                    }
                    self.master_next(cx, key);
                }
            }
            NbrState::Exchange => {
                if master {
                    if !ms && seq == dd_seq {
                        self.want_headers(cx, key, &headers);
                        if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
                            n.their_more = more;
                        }
                        self.master_next(cx, key);
                    }
                } else if ms {
                    if seq == dd_seq {
                        self.resend_dbd(cx, key);
                    } else if seq == dd_seq.wrapping_add(1) {
                        if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
                            n.dd_seq = seq;
                            n.their_more = more;
                        }
                        self.want_headers(cx, key, &headers);
                        self.send_dbd(cx, key, false);
                        let finished = self.ospf_rt.nbrs.get(key).is_some_and(|n| !n.sent_more && !n.their_more);
                        if finished {
                            self.exchange_done(cx, key);
                        }
                    }
                }
            }
            _ => {
                if !master && ms && seq == dd_seq {
                    self.resend_dbd(cx, key);
                }
            }
        }
        self.ospf_arm(key, cx.rxmt, true);
    }

    fn master_next(&mut self, cx: &Cx, key: &NKey) {
        let Some((sent_more, their_more)) = self.ospf_rt.nbrs.get(key).map(|n| (n.sent_more, n.their_more)) else { return };
        if !sent_more && !their_more {
            self.exchange_done(cx, key);
        } else {
            self.send_dbd(cx, key, true);
        }
    }

    /// Both descriptions are complete: request what is missing, or the neighbour is Full.
    fn exchange_done(&mut self, cx: &Cx, key: &NKey) {
        let pending = self.ospf_rt.nbrs.get(key).map_or(0, |n| n.request.len());
        if pending == 0 {
            self.ospf_set_state(key, NbrState::Full, "the databases are the same, nothing to request");
            self.on_full(cx, key);
        } else {
            self.ospf_set_state(
                key,
                NbrState::Loading,
                &format!("requesting {pending} LSA{} the neighbour has", if pending == 1 { "" } else { "s" }),
            );
            self.send_requests(cx, key);
        }
    }

    fn send_requests(&mut self, cx: &Cx, key: &NKey) {
        let list: Vec<Key> = self.ospf_rt.nbrs.get(key).map(|n| n.request.iter().copied().collect()).unwrap_or_default();
        let Some(to) = self.nbr_dest(key) else { return };
        for chunk in list.chunks(REQ_BATCH) {
            let req = OspfDb::Request(chunk.iter().map(|(k, id, adv)| (u32::from(*k), *id, *adv)).collect());
            self.ospf_tx(cx, to, &req);
        }
    }

    fn on_full(&mut self, cx: &Cx, key: &NKey) {
        if self.debug_on(cx.d.id, DebugKind::OspfEvents) {
            let routes = cx.st.routes.get(&cx.d.id).map_or(0, Vec::len);
            self.debug_raw(
                cx.d.id,
                DebugKind::OspfEvents,
                &format!("OSPF event: SPF run after the adjacency with {} came up: {routes} OSPF route{}", key.2, if routes == 1 { "" } else { "s" }),
            );
        }
        self.ospf_arm(key, cx.rxmt, false);
    }

    fn rx_lsr(&mut self, cx: &Cx, key: &NKey, list: Vec<(u32, Ipv4Addr, Ipv4Addr)>) {
        let now = self.now;
        let mut out: Vec<RawLsa> = Vec::new();
        for (k, id, adv) in list {
            let Ok(kind) = u8::try_from(k) else { continue };
            let hk: HeldKey = (lsa_area(kind, cx.area), kind, id, adv);
            match self.ospf_rt.held.get(&cx.d.id).and_then(|m| m.get(&hk)) {
                Some(h) => out.push(h.lsa.with_age(h.age(now).saturating_add(cx.delay).min(MAX_AGE))),
                None => self.debug_raw(
                    cx.d.id,
                    DebugKind::OspfFlood,
                    &format!("OSPF flood: {} asked for {} LSA {id} advertised by {adv}, which this router does not have", key.2, kind_word(kind)),
                ),
            }
        }
        self.send_lsas(cx, key, out);
    }

    /// Sends LSAs to one neighbour and keeps them on its retransmission list until acknowledged.
    fn send_lsas(&mut self, cx: &Cx, key: &NKey, lsas: Vec<RawLsa>) {
        if lsas.is_empty() {
            return;
        }
        let now = self.now;
        let Some(to) = self.nbr_dest(key) else { return };
        if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
            for l in &lsas {
                n.retrans.insert((l.header.kind, l.header.id, l.header.advertising), (l.clone(), now));
            }
        }
        for chunk in lsas.chunks(LSU_BATCH) {
            self.ospf_tx(cx, to, &OspfDb::Update(chunk.to_vec()));
        }
        if self.debug_on(cx.d.id, DebugKind::OspfFlood) {
            self.debug_raw(
                cx.d.id,
                DebugKind::OspfFlood,
                &format!("OSPF flood: {} LSA{} sent to {} on {}", lsas.len(), if lsas.len() == 1 { "" } else { "s" }, key.2, key.1),
            );
        }
        self.ospf_arm(key, cx.rxmt, false);
    }

    fn rx_lsu(&mut self, cx: &Cx, key: &NKey, lsas: Vec<RawLsa>) {
        if let Some(left) = self.ospf_rt.lose.get_mut(&cx.d.id).filter(|n| **n > 0) {
            *left -= 1;
            self.debug_raw(cx.d.id, DebugKind::OspfFlood, &format!("OSPF flood: update from {} on {} lost on the way", key.2, key.1));
            return;
        }
        let now = self.now;
        let mut acks: Vec<LsaHeader> = Vec::new();
        let mut fresh: Vec<(HeldKey, RawLsa)> = Vec::new();
        let mut back: Vec<RawLsa> = Vec::new();
        let verbose = self.debug_on(cx.d.id, DebugKind::OspfFlood);
        let mut notes: Vec<String> = Vec::new();
        for l in lsas {
            let h = l.header;
            if !matches!(h.kind, 1..=5 | 7) {
                continue;
            }
            let hk: HeldKey = (lsa_area(h.kind, cx.area), h.kind, h.id, h.advertising);
            if !in_scope(cx.d, cx.area, &hk) {
                continue;
            }
            // This router's own LSA coming back is acknowledged and not kept: the router that
            // originates it is the one that decides what it says.
            if h.advertising == cx.rid {
                acks.push(h);
                continue;
            }
            let nk: Key = (h.kind, h.id, h.advertising);
            let cur = self.ospf_rt.held.get(&cx.d.id).and_then(|m| m.get(&hk)).map(|x| (x.lsa.header.seq, x.age(now)));
            let newer = match cur {
                None => h.age < MAX_AGE,
                Some((s, a)) => seq_newer(h.seq, s) || (h.seq == s && h.age >= MAX_AGE && a < MAX_AGE),
            };
            if newer {
                acks.push(h);
                let held = self.ospf_rt.held.entry(cx.d.id).or_default();
                if h.age >= MAX_AGE {
                    held.remove(&hk);
                } else if held.len() < MAX_HELD || held.contains_key(&hk) {
                    held.insert(hk, Held { lsa: l.clone(), at: now });
                }
                if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
                    n.request.retain(|r| *r != nk);
                    n.retrans.remove(&nk);
                }
                if verbose {
                    notes.push(format!(
                        "OSPF flood: {} received from {}, {}",
                        describe(&h),
                        key.2,
                        if h.age >= MAX_AGE { "flushed from the database" } else { "installed" }
                    ));
                }
                fresh.push((hk, l));
            } else if cur.is_some_and(|(s, _)| s == h.seq) {
                // A copy already held: when it was waiting for an acknowledgement from this
                // neighbour, it counts as one.
                let implied = self.ospf_rt.nbrs.get_mut(key).is_some_and(|n| n.retrans.remove(&nk).is_some());
                if !implied {
                    acks.push(h);
                }
                if verbose {
                    notes.push(format!("OSPF flood: {} received again from {}, already held", describe(&h), key.2));
                }
            } else if let Some(m) = self.ospf_rt.held.get(&cx.d.id).and_then(|m| m.get(&hk)) {
                // This router has a newer instance: the neighbour gets it back.
                back.push(m.lsa.with_age(m.age(now).saturating_add(cx.delay).min(MAX_AGE)));
            }
        }
        for n in notes {
            self.debug_raw(cx.d.id, DebugKind::OspfFlood, &n);
        }
        if !acks.is_empty()
            && let Some(to) = self.nbr_dest(key)
        {
            for chunk in acks.chunks(DBD_BATCH) {
                self.ospf_tx(cx, to, &OspfDb::Ack(chunk.to_vec()));
            }
        }
        self.send_lsas(cx, key, back);
        if !fresh.is_empty() {
            self.ospf_flood(cx.t, cx.st, cx.d.id, fresh, Some((cx.ifc.name.clone(), key.2)));
        }
        let loaded = self.ospf_rt.nbrs.get(key).is_some_and(|n| n.state == NbrState::Loading && n.request.is_empty());
        if loaded {
            self.ospf_set_state(key, NbrState::Full, "every requested LSA has arrived");
            self.on_full(cx, key);
        }
        self.ospf_arm(key, cx.rxmt, false);
    }

    fn rx_ack(&mut self, cx: &Cx, key: &NKey, headers: Vec<LsaHeader>) {
        let mut done = 0usize;
        if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
            for h in &headers {
                let nk: Key = (h.kind, h.id, h.advertising);
                if n.retrans.get(&nk).is_some_and(|(inst, _)| inst.header.seq == h.seq) {
                    n.retrans.remove(&nk);
                    done += 1;
                }
            }
        }
        if done > 0 && self.debug_on(cx.d.id, DebugKind::OspfFlood) {
            self.debug_raw(
                cx.d.id,
                DebugKind::OspfFlood,
                &format!("OSPF flood: {} acknowledged {done} LSA{} on {}", key.2, if done == 1 { "" } else { "s" }, key.1),
            );
        }
        self.ospf_arm(key, cx.rxmt, false);
    }

    // ---- flooding ----

    /// Floods LSAs out of every interface of `dev` that has an adjacent neighbour, except back to
    /// the neighbour they came from (RFC 2328 §13.3). On a broadcast segment an update from the DR or
    /// BDR is not sent again there, and only the DR sends one on that came from another router.
    pub(crate) fn ospf_flood(
        &mut self,
        t: &Topology,
        st: &OspfState,
        dev: DeviceId,
        lsas: Vec<(HeldKey, RawLsa)>,
        except: Option<(String, Ipv4Addr)>,
    ) {
        let now = self.now;
        let ifaces: BTreeSet<String> =
            self.ospf_rt.nbrs.iter().filter(|(k, n)| k.0 == dev && n.state >= NbrState::Exchange).map(|(k, _)| k.1.clone()).collect();
        for iface in ifaces {
            let Some(cx) = context(t, st, dev, &iface) else { continue };
            let bcast = cx.seg.is_some_and(|s| s.dr.is_some());
            let mine = cx.ifc.net.addr;
            let am_dr = cx.seg.is_some_and(|s| s.dr == Some(mine));
            let am_bdr = cx.seg.is_some_and(|s| s.bdr == Some(mine));
            if let Some((ex_if, ex_rid)) = &except
                && *ex_if == iface
                && bcast
            {
                let from = self.ospf_rt.nbrs.get(&(dev, iface.clone(), *ex_rid)).map(|n| n.addr);
                let from_dr_or_bdr = cx.seg.is_some_and(|s| from.is_some() && (s.dr == from || s.bdr == from));
                if from_dr_or_bdr || !am_dr {
                    continue;
                }
            }
            let list: Vec<RawLsa> = lsas
                .iter()
                .filter(|(hk, _)| in_scope(cx.d, cx.area, hk))
                .map(|(_, l)| l.with_age(l.header.age.saturating_add(cx.delay).min(MAX_AGE)))
                .collect();
            if list.is_empty() {
                continue;
            }
            let targets: Vec<NKey> = self
                .ospf_rt
                .nbrs
                .iter()
                .filter(|(k, n)| k.0 == dev && k.1 == iface && n.state >= NbrState::Exchange && except.as_ref().is_none_or(|(_, r)| *r != k.2))
                .map(|(k, _)| k.clone())
                .collect();
            if targets.is_empty() {
                continue;
            }
            for k in &targets {
                if let Some(n) = self.ospf_rt.nbrs.get_mut(k) {
                    for l in &list {
                        n.retrans.insert((l.header.kind, l.header.id, l.header.advertising), (l.clone(), now));
                    }
                }
                self.ospf_arm(k, cx.rxmt, false);
            }
            let dst = if !bcast || am_dr || am_bdr { Dest::AllSpf } else { Dest::AllDr };
            for chunk in list.chunks(LSU_BATCH) {
                self.ospf_tx(&cx, dst, &OspfDb::Update(chunk.to_vec()));
            }
            if self.debug_on(dev, DebugKind::OspfFlood) {
                self.debug_raw(
                    dev,
                    DebugKind::OspfFlood,
                    &format!(
                        "OSPF flood: update with {} LSA{} sent out of {iface} to {} neighbour{}",
                        list.len(),
                        if list.len() == 1 { "" } else { "s" },
                        targets.len(),
                        if targets.len() == 1 { "" } else { "s" }
                    ),
                );
            }
        }
    }

    /// Every router originates the LSAs the lab gives it: a new or changed one goes into the
    /// router's copy of the database with its new sequence number and is flooded, one that is no
    /// longer there is flushed (flooded at MaxAge), and one the router has held for half an hour is
    /// refreshed. Called with the sequence numbers (the stamps) already brought up to date.
    pub(crate) fn ospf_originate(&mut self, t: &Topology, st: &OspfState) {
        let now = self.now;
        for (dev, rid) in &st.router_ids {
            let mut own: Vec<Lsa> = ospf_lsdb::database(t, st, *dev).into_iter().filter(|l| l.adv == *rid).collect();
            ospf_lsdb::apply_stamps(&self.lsa_stamps, now, &mut own);
            let mut keep: BTreeSet<HeldKey> = BTreeSet::new();
            let mut fresh: Vec<(HeldKey, RawLsa)> = Vec::new();
            let held = self.ospf_rt.held.entry(*dev).or_default();
            for l in &own {
                let raw = to_raw(l, 0);
                let key: HeldKey = (l.area, raw.header.kind, l.id, l.adv);
                keep.insert(key);
                let changed = held.get(&key).is_none_or(|h| h.lsa.header.seq != raw.header.seq || h.lsa.body != raw.body);
                if changed {
                    held.insert(key, Held { lsa: raw.clone(), at: now });
                    fresh.push((key, raw));
                } else if let Some(h) = held.get_mut(&key)
                    && now >= h.at.saturating_add(LS_REFRESH_US)
                {
                    // Nothing says it again at the same number: the age starts over.
                    h.at = now;
                }
            }
            let gone: Vec<HeldKey> = held.iter().filter(|(k, _)| k.3 == *rid && !keep.contains(*k)).map(|(k, _)| *k).collect();
            for k in gone {
                if let Some(h) = held.remove(&k) {
                    fresh.push((k, h.lsa.with_age(MAX_AGE)));
                }
            }
            if !fresh.is_empty() {
                self.ospf_flood(t, st, *dev, fresh, None);
            }
        }
        self.ospf_rt.held.retain(|d, _| st.router_ids.contains_key(d));
        self.ospf_rt.hold_due = self.ospf_hold_due(&st.router_ids);
    }

    // ---- timers ----

    /// The earliest time a copy is due to be refreshed (the router's own) or to age out (anyone
    /// else's).
    fn ospf_hold_due(&self, rids: &BTreeMap<DeviceId, Ipv4Addr>) -> Option<u64> {
        let mut best: Option<u64> = None;
        for (dev, map) in &self.ospf_rt.held {
            let rid = rids.get(dev);
            for (k, h) in map {
                let due = if Some(&k.3) == rid {
                    h.at.saturating_add(LS_REFRESH_US)
                } else {
                    h.at.saturating_add(u64::from(MAX_AGE.saturating_sub(h.lsa.header.age)) * 1_000_000)
                };
                best = min_opt(best, Some(due));
            }
        }
        best
    }

    /// The next time something in OSPF's flooding is due: a hello, a packet to send again, a
    /// refresh or an LSA reaching MaxAge.
    pub(crate) fn ospf_next_expiry(&self) -> Option<u64> {
        let mut best = self.ospf_rt.hello_due.iter().next().map(|h| h.0);
        best = min_opt(best, self.ospf_rt.nbrs.values().filter_map(|n| n.due).min());
        min_opt(best, self.ospf_rt.hold_due)
    }

    /// Runs what is due at the current time.
    pub(crate) fn ospf_fire(&mut self, t: &Topology) {
        let now = self.now;
        let hellos: Vec<(u64, DeviceId, String)> = self.ospf_rt.hello_due.iter().filter(|h| h.0 <= now).cloned().collect();
        for h in &hellos {
            self.ospf_rt.hello_due.remove(h);
        }
        let due: Vec<NKey> = self.ospf_rt.nbrs.iter().filter(|(_, n)| n.due.is_some_and(|x| x <= now)).map(|(k, _)| k.clone()).collect();
        let hold = self.ospf_rt.hold_due.is_some_and(|x| x <= now);
        self.with_ospf(t, |s, st| {
            for (at, dev, name) in &hellos {
                if let Some(d) = t.device(*dev) {
                    s.ospf_hello(t, st, d, name, *at);
                }
            }
            for k in &due {
                s.ospf_rxmt(t, st, k);
            }
            if hold {
                s.ospf_age_out(now, &st.router_ids);
                s.ospf_originate(t, st);
            }
        });
        // Never spin on a timer that was not cleared.
        for n in self.ospf_rt.nbrs.values_mut() {
            if n.due.is_some_and(|x| x <= now) {
                n.due = Some(now.saturating_add(1_000_000));
            }
        }
        if self.ospf_rt.hold_due.is_some_and(|x| x <= now) {
            self.ospf_rt.hold_due = Some(now.saturating_add(1_000_000));
        }
    }

    /// Copies other routers originated leave the database when their age reaches MaxAge.
    fn ospf_age_out(&mut self, now: u64, rids: &BTreeMap<DeviceId, Ipv4Addr>) {
        for (dev, map) in &mut self.ospf_rt.held {
            let rid = rids.get(dev).copied();
            map.retain(|k, h| Some(k.3) == rid || h.age(now) < MAX_AGE);
        }
    }

    /// A neighbour's retransmission timer ran out: send again what it has not answered.
    fn ospf_rxmt(&mut self, t: &Topology, st: &OspfState, key: &NKey) {
        let Some(cx) = context(t, st, key.0, &key.1) else { return };
        let now = self.now;
        let Some(n) = self.ospf_rt.nbrs.get(key) else { return };
        let (state, master) = (n.state, n.master);
        let waiting: Vec<(RawLsa, u64)> = n.retrans.values().cloned().collect();
        let requests = n.request.len();
        let mut said: Vec<String> = Vec::new();
        if state == NbrState::ExStart || (state == NbrState::Exchange && master) {
            self.resend_dbd(&cx, key);
            said.push("database description".to_string());
        }
        if state == NbrState::Loading && requests > 0 {
            self.send_requests(&cx, key);
            said.push(format!("link-state request for {requests} LSA{}", if requests == 1 { "" } else { "s" }));
        }
        if !waiting.is_empty() {
            let lsas: Vec<RawLsa> = waiting.iter().map(|(l, at)| l.with_age(age_at(l.header.age, *at, now))).collect();
            if let Some(to) = self.nbr_dest(key) {
                for chunk in lsas.chunks(LSU_BATCH) {
                    self.ospf_tx(&cx, to, &OspfDb::Update(chunk.to_vec()));
                }
            }
            if self.debug_on(key.0, DebugKind::OspfRxmt) {
                for (l, _) in &waiting {
                    self.debug_raw(
                        key.0,
                        DebugKind::OspfRxmt,
                        &format!("OSPF retransmission: {} sent again to {} on {}, not acknowledged", describe(&l.header), key.2, key.1),
                    );
                }
            }
            said.push(format!("update with {} LSA{}", waiting.len(), if waiting.len() == 1 { "" } else { "s" }));
        }
        if !said.is_empty() {
            if let Some(n) = self.ospf_rt.nbrs.get_mut(key) {
                n.rxmt_sent = n.rxmt_sent.saturating_add(1);
            }
            self.debug_raw(
                key.0,
                DebugKind::OspfRxmt,
                &format!("OSPF retransmission: neighbour {} on {}: {} sent again after {} s", key.2, key.1, said.join(" and "), cx.rxmt / 1_000_000),
            );
        }
        self.ospf_arm(key, cx.rxmt, true);
    }
}
