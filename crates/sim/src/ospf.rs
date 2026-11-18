//! OSPF (RFC 2328) with areas, computed the way the protocol converges.
//!
//! Interfaces whose address matches a `network … area` statement run OSPF. Two routers are
//! neighbours when such interfaces (not passive) share a subnet and reach each other at layer 2.
//! Every router runs SPF (Dijkstra) over the neighbour graph with interface costs (reference
//! bandwidth 100 Mbit/s: Fast and Gigabit Ethernet cost 1) and installs the networks other routers
//! advertise. A router with `default-information originate` and a static default route
//! advertises it (type 2 external, metric 1). Routes other protocols hand it (`redistribute`, see
//! [`crate::redist`]) are external routes of type 1 (the cost to the advertising router is added to the
//! metric) or type 2 (the metric alone), and an internal route always beats an external one.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::{Device, DeviceKind, End, NextHop, OspfArea, OspfAuthKind, Topology, VirtualLinkOpts};
use netlabcraft_wire::control::{OSPF_OPT_E, OSPF_OPT_N, OspfAuth, OspfAuthSend};

use crate::{L3If, port_up, up_ifaces};

/// A route OSPF installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfRoute {
    pub net: Ipv4Net,
    pub via: Ipv4Addr,
    pub iface: String,
    pub cost: u32,
    /// An external route: a redistributed route or the default route.
    pub external: bool,
    /// An external route of type 1 (its cost includes the way to the router that advertises it).
    pub type1: bool,
    /// The network is in an area this router has no interface in (an inter-area route).
    pub inter_area: bool,
    /// An external route heard as a type 7 LSA inside a not-so-stubby area (`O N1`, `O N2`).
    pub nssa: bool,
    /// The tag of an external route (0: none): what the external LSA carries.
    pub tag: u32,
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
    /// The metric and whether it is type 1, of each redistributed external route a router injects.
    pub external_metrics: BTreeMap<(DeviceId, Ipv4Net), (u32, bool)>,
    /// The virtual links each router is configured with (`area N virtual-link`), and whether
    /// they work.
    pub virtual_links: BTreeMap<DeviceId, Vec<VirtualLink>>,
    /// The router that translates each NSSA's type 7 LSAs into type 5 (RFC 3101 §3.1): the area
    /// border router with the highest router ID.
    pub translators: BTreeMap<u32, DeviceId>,
    /// The tag of each redistributed external route a router injects (0 is left out).
    pub external_tags: BTreeMap<(DeviceId, Ipv4Net), u32>,
}

/// One `area N virtual-link ROUTER-ID` line, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualLink {
    /// The transit area.
    pub area: u32,
    /// The router ID the line names.
    pub peer_id: Ipv4Addr,
    /// The router that has that ID, when there is one.
    pub peer: Option<DeviceId>,
    pub up: bool,
    /// The cost of the way across the transit area (when up).
    pub cost: u32,
    /// Why the link is down (empty when up).
    pub why: String,
    /// The link's hello and dead intervals, in seconds.
    pub hello: u16,
    pub dead: u16,
    /// The retransmit interval and the transmit delay, in seconds (kept and shown; nothing is
    /// retransmitted here).
    pub retransmit: u16,
    pub delay: u16,
    /// The authentication the link uses: its own setting, else its transit area's.
    pub auth: OspfAuthKind,
}

/// The options of the virtual link to `peer` in an area's settings (all defaults when none).
pub fn vlink_opts(area: &OspfArea, peer: Ipv4Addr) -> VirtualLinkOpts {
    area.vlink_opts.iter().find(|v| v.peer == peer).cloned().unwrap_or_else(|| VirtualLinkOpts::new(peer))
}

/// The authentication type of a virtual link: its own, else the transit area's, else none.
pub fn vlink_auth(area: &OspfArea, opts: &VirtualLinkOpts) -> OspfAuthKind {
    opts.auth.or(area.auth).unwrap_or(OspfAuthKind::Null)
}

/// Why the two ends of a virtual link do not agree: the hello or dead interval differs, or the
/// authentication (type, password or message-digest key) does. `mine` is this router's line to
/// `my_peer`, `theirs` the far router's line back.
fn vlink_mismatch(mine: &OspfArea, my_peer: Ipv4Addr, theirs: &OspfArea, their_peer: Ipv4Addr) -> Option<String> {
    let (m, t) = (vlink_opts(mine, my_peer), vlink_opts(theirs, their_peer));
    let ((mh, md), (th, td)) = (m.timers(), t.timers());
    if mh != th {
        return Some(format!("the hello interval differs ({mh} s here, {th} s at the far end)"));
    }
    if md != td {
        return Some(format!("the dead interval differs ({md} s here, {td} s at the far end)"));
    }
    let (mk, tk) = (vlink_auth(mine, &m), vlink_auth(theirs, &t));
    if mk != tk {
        return Some(format!("authentication differs: {} here, {} at the far end", kind_name(mk), kind_name(tk)));
    }
    match mk {
        OspfAuthKind::Null => None,
        OspfAuthKind::Simple => {
            let key = |o: &VirtualLinkOpts| o.auth_key.as_deref().unwrap_or("").chars().take(8).collect::<String>();
            (key(&m) != key(&t)).then(|| "authentication fails: the clear-text password differs".to_string())
        }
        OspfAuthKind::Md5 => {
            let (Some((mi, mkey)), Some((ti, tkey))) = (m.md5_keys.iter().next_back(), t.md5_keys.iter().next_back()) else {
                return Some("message-digest authentication needs a message-digest-key on both ends".into());
            };
            if t.md5_keys.get(mi) != Some(mkey) {
                return Some(format!("authentication fails: the far end has no message-digest key {mi} with the same key"));
            }
            (m.md5_keys.get(ti) != Some(tkey)).then(|| format!("authentication fails: this router has no message-digest key {ti} with the same key"))
        }
    }
}

/// What an area is, from a router's configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AreaKind {
    Normal,
    Stub,
    Nssa,
}

impl AreaKind {
    /// The name `show` commands and messages use.
    pub fn word(self) -> &'static str {
        match self {
            AreaKind::Normal => "normal area",
            AreaKind::Stub => "stub area",
            AreaKind::Nssa => "NSSA",
        }
    }

    /// The hello options byte of an interface in an area of this kind (E bit in a normal area,
    /// N bit in an NSSA, neither in a stub area).
    pub fn options(self) -> u8 {
        match self {
            AreaKind::Normal => OSPF_OPT_E,
            AreaKind::Stub => 0,
            AreaKind::Nssa => OSPF_OPT_N,
        }
    }

    /// The kind of area a hello's options byte announces.
    pub fn from_options(options: u8) -> AreaKind {
        if options & OSPF_OPT_E != 0 {
            AreaKind::Normal
        } else if options & OSPF_OPT_N != 0 {
            AreaKind::Nssa
        } else {
            AreaKind::Stub
        }
    }
}

/// The kind of area `area` is under an OSPF process's configuration (the backbone is always normal).
pub fn kind_of(o: &netlabcraft_model::Ospf, area: u32) -> AreaKind {
    match o.areas.get(&area) {
        Some(_) if area == 0 => AreaKind::Normal,
        Some(a) if a.stub => AreaKind::Stub,
        Some(a) if a.nssa => AreaKind::Nssa,
        _ => AreaKind::Normal,
    }
}

/// The kind of area `area` is on a router.
pub fn area_kind(d: &Device, area: u32) -> AreaKind {
    d.ospf.as_ref().map_or(AreaKind::Normal, |o| kind_of(o, area))
}

/// The options byte this router puts in the hellos it sends on an interface.
pub fn hello_options(d: &Device, ifc: &L3If) -> u8 {
    area_kind(d, area_of(d, ifc)).options()
}

/// Why two ends cannot be neighbours over their area types, if they cannot: a stub area has no
/// E bit, an NSSA has the N bit instead, so the hello options differ (RFC 2328 §10.5).
pub fn kind_problem(mine: AreaKind, theirs: AreaKind) -> Option<String> {
    let bit = |o: u8, b: u8| u8::from(o & b != 0);
    (mine != theirs).then(|| {
        format!(
            "the area is a {} here and a {} there: the hello options differ (E bit {}, N bit {} here; E bit {}, N bit {} there)",
            mine.word(),
            theirs.word(),
            bit(mine.options(), OSPF_OPT_E),
            bit(mine.options(), OSPF_OPT_N),
            bit(theirs.options(), OSPF_OPT_E),
            bit(theirs.options(), OSPF_OPT_N)
        )
    })
}

/// The area border router that translates an NSSA's type 7 LSAs into type 5: among the routers
/// in the area that also have an interface in another area, the one with the highest router ID
/// (RFC 3101 §3.1); a border router set to `translate type7 always` comes first, the highest
/// router ID among those. (RFC 3101 lets every such router translate, and this simulator keeps
/// one translator per area.)
pub fn translator_of(
    t: &Topology,
    areas_of: &BTreeMap<DeviceId, BTreeSet<u32>>,
    router_ids: &BTreeMap<DeviceId, Ipv4Addr>,
    area: u32,
) -> Option<DeviceId> {
    let candidates: Vec<(bool, Ipv4Addr, DeviceId)> = areas_of
        .iter()
        .filter(|(d, a)| a.contains(&area) && a.len() > 1 && t.device(**d).is_some_and(|x| area_kind(x, area) == AreaKind::Nssa))
        .filter_map(|(d, _)| {
            let always = t.device(*d).and_then(|x| x.ospf.as_ref()).is_some_and(|o| o.area(area).nssa_translate_always);
            router_ids.get(d).map(|r| (always, *r, *d))
        })
        .collect();
    candidates.into_iter().max().map(|(_, _, d)| d)
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
    d.ospf_if.get(name).cloned().unwrap_or_default()
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
    if h.area != area {
        return Some(format!("area {} in the hello, here area {area}", h.area));
    }
    kind_problem(area_kind(d, area), AreaKind::from_options(h.options))
}

/// The authentication type an interface uses: its own setting, a key chain it names, else its
/// area's, else none.
pub fn auth_kind(d: &Device, ifc: &L3If) -> OspfAuthKind {
    let c = if_conf(d, &ifc.name);
    c.auth
        .or_else(|| c.key_chain.as_ref().map(|_| OspfAuthKind::Md5))
        .or_else(|| d.ospf.as_ref().and_then(|o| o.areas.get(&area_of(d, ifc))).and_then(|a| a.auth))
        .unwrap_or(OspfAuthKind::Null)
}

fn kind_name(k: OspfAuthKind) -> &'static str {
    match k {
        OspfAuthKind::Null => "none",
        OspfAuthKind::Simple => "simple password",
        OspfAuthKind::Md5 => "message digest",
    }
}

/// The time the analytic OSPF state reads a device's key chains against (see
/// [`Device::clock_unix`]).
pub fn analysis_clock(d: &Device) -> i64 {
    d.clock_unix.unwrap_or(crate::DEFAULT_CLOCK)
}

/// The message-digest keys an interface signs with at `now` (Unix seconds), newest first: every
/// `message-digest-key` it holds, or the one key of its key chain that signs now. A router
/// that holds several sends one copy of each packet per key, so a neighbour that has only an
/// older key still hears it while the keys are being rolled over.
pub fn md5_sign_keys<'a>(d: &'a Device, name: &str, now: i64) -> Vec<(u8, &'a str)> {
    let Some(c) = d.ospf_if.get(name) else { return Vec::new() };
    match &c.key_chain {
        Some(chain) => d.key_chains.get(chain).and_then(|k| k.signing(now)).into_iter().collect(),
        None => c.md5_keys.iter().rev().map(|(id, k)| (*id, k.as_str())).collect(),
    }
}

/// The key a packet that names key `id` is checked against at `now`, if the interface accepts
/// that key.
pub fn md5_accept_key<'a>(d: &'a Device, name: &str, now: i64, id: u8) -> Option<&'a str> {
    let c = d.ospf_if.get(name)?;
    match &c.key_chain {
        Some(chain) => d.key_chains.get(chain)?.accepting(now).into_iter().find(|(n, _)| *n == id).map(|(_, k)| k),
        None => c.md5_keys.get(&id).map(String::as_str),
    }
}

/// What this interface signs its OSPF packets with, `at` being the simulated time in
/// microseconds (the digest's sequence number counts seconds and never goes back) and `now` the
/// device's clock in Unix seconds (a key chain's lifetimes read it). One entry per copy of the
/// packet to send: several when message-digest keys are being rolled over. An interface set to
/// message digests with no usable key sends unsigned packets, which no neighbour accepts.
pub fn auth_sends<'a>(d: &'a Device, ifc: &L3If, at: u64, now: i64) -> Vec<OspfAuthSend<'a>> {
    let c = d.ospf_if.get(&ifc.name);
    match auth_kind(d, ifc) {
        OspfAuthKind::Null => vec![OspfAuthSend::Null],
        OspfAuthKind::Simple => vec![OspfAuthSend::Simple(c.and_then(|c| c.auth_key.as_deref()).unwrap_or(""))],
        OspfAuthKind::Md5 => {
            let seq = u32::try_from(at / 1_000_000).unwrap_or(u32::MAX).max(1);
            let keys = md5_sign_keys(d, &ifc.name, now);
            if keys.is_empty() {
                vec![OspfAuthSend::Null]
            } else {
                keys.into_iter().map(|(key_id, key)| OspfAuthSend::Md5 { key_id, key, seq }).collect()
            }
        }
    }
}

/// Why an OSPF packet that arrived on `ifc` fails this router's authentication, if it does:
/// the type differs from the interface's, the password differs, or the digest does not verify
/// under the key the packet names (the digest is recomputed over the packet). `now` is the
/// device's clock in Unix seconds, which a key chain's accept lifetimes read.
pub fn auth_problem(d: &Device, ifc: &L3If, packet: &[u8], now: i64) -> Option<String> {
    let want = auth_kind(d, ifc);
    let c = if_conf(d, &ifc.name);
    let Some(got) = netlabcraft_wire::control::ospf_auth_of(packet) else { return Some("unknown authentication type in the packet".into()) };
    match (want, got) {
        (OspfAuthKind::Null, OspfAuth::Null) => None,
        (OspfAuthKind::Simple, OspfAuth::Simple(pw)) => {
            let mut mine = [0u8; 8];
            for (m, b) in mine.iter_mut().zip(c.auth_key.as_deref().unwrap_or("").bytes()) {
                *m = b;
            }
            (mine != pw).then(|| "the clear-text password differs".to_string())
        }
        (OspfAuthKind::Md5, OspfAuth::Md5 { key_id, .. }) => match md5_accept_key(d, &ifc.name, now, key_id) {
            None => Some(match &c.key_chain {
                Some(chain) => format!("key chain {chain} has no key {key_id} valid for receiving now"),
                None => format!("no message-digest key {key_id} on the receiving interface"),
            }),
            Some(key) => {
                (!netlabcraft_wire::control::ospf_md5_verify(packet, key)).then(|| format!("the message digest does not verify under key {key_id}"))
            }
        },
        (w, g) => {
            let theirs = match g {
                OspfAuth::Null => OspfAuthKind::Null,
                OspfAuth::Simple(_) => OspfAuthKind::Simple,
                OspfAuth::Md5 { .. } => OspfAuthKind::Md5,
            };
            Some(format!("the packet uses {} authentication, the receiving interface expects {}", kind_name(theirs), kind_name(w)))
        }
    }
}

/// Why a message-digest packet is a replay: the sequence number it carries is below the highest
/// one already heard from the same sender under the same key (`last`). Equal numbers pass, since
/// several packets leave in one second. A packet that is not a message-digest one is never a
/// replay.
pub fn replay_problem(last: Option<u32>, packet: &[u8]) -> Option<String> {
    let OspfAuth::Md5 { seq, .. } = netlabcraft_wire::control::ospf_auth_of(packet)? else { return None };
    let last = last?;
    (seq < last).then(|| format!("sequence number {seq} is below {last}, the last one heard: a replayed packet"))
}

/// One direction of the authentication check between two routers on a link: a probe signed as
/// `from` would sign it (once per key, as the packet is sent) is checked the way `to` checks
/// it, and one copy that passes is enough. The reason, when none does, is the newest key's.
fn one_way(from: &Device, ifa: &L3If, to: &Device, ifb: &L3If, probe: &[u8]) -> Option<String> {
    let (now_from, now_to) = (analysis_clock(from), analysis_clock(to));
    let mut first = None;
    for sign in auth_sends(from, ifa, 1_000_000, now_from) {
        let packet = netlabcraft_wire::control::ospf_authenticate(probe.to_vec(), &sign);
        match auth_problem(to, ifb, &packet, now_to) {
            None => return None,
            Some(why) => {
                first.get_or_insert(why);
            }
        }
    }
    first
}

/// Why two routers on one link can't form an adjacency over authentication: a hello of each,
/// signed as its sender would sign it, is checked the way the other end checks it.
fn auth_mismatch(a: &Device, ia: &L3If, b: &Device, ib: &L3If) -> Option<String> {
    if auth_kind(a, ia) == OspfAuthKind::Null && auth_kind(b, ib) == OspfAuthKind::Null {
        return None;
    }
    let probe = netlabcraft_wire::control::OspfHello {
        options: OSPF_OPT_E,
        router_id: Ipv4Addr::UNSPECIFIED,
        area: 0,
        mask: Ipv4Addr::UNSPECIFIED,
        hello_secs: 10,
        dead_secs: 40,
        priority: 1,
        dr: Ipv4Addr::UNSPECIFIED,
        bdr: Ipv4Addr::UNSPECIFIED,
        neighbors: Vec::new(),
    }
    .encode();
    if let Some(why) = one_way(a, ia, b, ib, &probe) {
        return Some(format!("authentication fails: the neighbour rejects this router's packets ({why})"));
    }
    one_way(b, ib, a, ia, &probe).map(|why| format!("authentication fails: this router rejects the neighbour's packets ({why})"))
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

/// Whether a router is attached to the backbone: an up interface with a neighbour in area 0, or
/// a working virtual link (which is a backbone adjacency). An area border router that is not
/// cannot pass routes between its areas.
pub fn backbone_attached(st: &OspfState, d: DeviceId) -> bool {
    st.neighbors.get(&d).into_iter().flatten().any(|n| st.segments.get(&(d, n.iface.clone())).is_some_and(|s| s.area == 0))
        || st.virtual_links.get(&d).into_iter().flatten().any(|v| v.up)
}

/// The adjacencies of each router: (neighbour, this router's interface, the neighbour's address, cost).
type Adjacencies = BTreeMap<DeviceId, Vec<(DeviceId, L3If, Ipv4Addr, u32)>>;

/// The cost of the best way from one router to another using only interfaces in `area`.
fn area_distance(adj: &Adjacencies, devs: &BTreeMap<DeviceId, &Device>, from: DeviceId, to: DeviceId, area: u32) -> Option<u32> {
    let mut dist: BTreeMap<DeviceId, u32> = BTreeMap::from([(from, 0)]);
    let mut done: BTreeSet<DeviceId> = BTreeSet::new();
    loop {
        let (u, cu) = dist.iter().filter(|(d, _)| !done.contains(*d)).min_by_key(|(d, c)| (**c, **d)).map(|(d, c)| (*d, *c))?;
        if u == to {
            return Some(cu);
        }
        done.insert(u);
        let Some(du) = devs.get(&u) else { continue };
        for (v, ifc, _, cost) in adj.get(&u).into_iter().flatten() {
            if done.contains(v) || area_of(du, ifc) != area {
                continue;
            }
            let c = cu.saturating_add(*cost);
            if dist.get(v).is_none_or(|old| c < *old) {
                dist.insert(*v, c);
            }
        }
    }
}

pub fn compute(t: &Topology) -> OspfState {
    compute_seeded(t, &crate::redist::seeds(t))
}

/// A router's OSPF distribute lists.
fn dist_of(d: &Device) -> &[netlabcraft_model::redist::DistList] {
    d.ospf.as_ref().map_or(&[][..], |o| o.distribute.as_slice())
}

/// [`compute`] with the routes other protocols give OSPF (`redistribute`) worked out already.
pub fn compute_seeded(t: &Topology, seeds: &crate::redist::Seeds) -> OspfState {
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
                    // Hellos must agree on the timers, the area and the area type (RFC 2328 §10.5).
                    let (ha, da) = timers(a, &ia.name);
                    let (hb, db) = timers(b, &ib.name);
                    let why = if (ha, da) != (hb, db) {
                        Some(format!("hello/dead intervals differ ({ha}/{da} s here, {hb}/{db} s there)"))
                    } else if area_of(a, ia) != area_of(b, ib) {
                        Some(format!("areas differ ({} here, {} there)", area_of(a, ia), area_of(b, ib)))
                    } else {
                        // The area type is in the hello's options (E and N bits).
                        kind_problem(area_kind(a, area_of(a, ia)), area_kind(b, area_of(b, ib))).or_else(|| auth_mismatch(a, ia, b, ib))
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
    // Virtual links (`area N virtual-link`): configured on both ends, across a transit area both
    // ends have an interface in, with a way between them inside that area.
    let devs: BTreeMap<DeviceId, &Device> = routers.iter().map(|d| (d.id, *d)).collect();
    for a in &routers {
        let (Some(o), Some(my_rid)) = (a.ospf.as_ref(), st.router_ids.get(&a.id).copied()) else { continue };
        for (area, conf) in &o.areas {
            for peer_id in &conf.virtual_links {
                let peer = st.router_ids.iter().find(|(_, r)| **r == *peer_id).map(|(d, _)| *d);
                let check = || -> Result<u32, String> {
                    if *area == 0 {
                        return Err("the transit area cannot be the backbone".into());
                    }
                    if kind_of(o, *area) != AreaKind::Normal {
                        return Err(format!("area {area} is a {}, which cannot carry a virtual link", kind_of(o, *area).word()));
                    }
                    if *peer_id == my_rid {
                        return Err("a virtual link goes to another router".into());
                    }
                    let p = peer.and_then(|id| devs.get(&id).copied()).ok_or_else(|| format!("no router with router ID {peer_id} runs OSPF here"))?;
                    if !p.ospf.as_ref().and_then(|po| po.areas.get(area)).is_some_and(|pc| pc.virtual_links.contains(&my_rid)) {
                        return Err(format!("router {peer_id} has no `area {area} virtual-link {my_rid}` line"));
                    }
                    let theirs = p.ospf.as_ref().map(|po| po.area(*area)).unwrap_or_default();
                    if let Some(why) = vlink_mismatch(conf, *peer_id, &theirs, my_rid) {
                        return Err(why);
                    }
                    let attached = |x: &Device| ifs.get(&x.id).into_iter().flatten().any(|(i, _)| area_of(x, i) == *area);
                    if !attached(a) {
                        return Err(format!("this router has no OSPF interface in transit area {area}"));
                    }
                    if !attached(p) {
                        return Err(format!("router {peer_id} has no OSPF interface in transit area {area}"));
                    }
                    area_distance(&adj, &devs, a.id, p.id, *area).ok_or_else(|| format!("no path between the two routers inside transit area {area}"))
                };
                let (up, cost, why) = match check() {
                    Ok(c) => (true, c, String::new()),
                    Err(w) => (false, 0, w),
                };
                let opts = vlink_opts(conf, *peer_id);
                let (hello, dead) = opts.timers();
                let link = VirtualLink {
                    area: *area,
                    peer_id: *peer_id,
                    peer,
                    up,
                    cost,
                    why,
                    hello,
                    dead,
                    retransmit: opts.retransmit.unwrap_or(5),
                    delay: opts.transmit_delay.unwrap_or(1),
                    auth: vlink_auth(conf, &opts),
                };
                st.virtual_links.entry(a.id).or_default().push(link);
            }
        }
    }
    // Routers attached to the backbone: an up interface with a neighbour in area 0, or a working
    // virtual link (which is a backbone adjacency).
    let backbone: BTreeSet<DeviceId> = routers.iter().map(|d| d.id).filter(|d| backbone_attached(&st, *d)).collect();
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
    // Routes redistributed into OSPF, per router (worked out from its routing table).
    let redistributed: BTreeMap<DeviceId, Vec<crate::redist::OspfSeed>> =
        routers.iter().filter_map(|d| seeds.ospf.get(&d.id).map(|v| (d.id, v.clone()))).collect();
    for (d, list) in &redistributed {
        st.externals.entry(*d).or_default().extend(list.iter().map(|x| x.net));
        for x in list {
            st.external_metrics.insert((*d, x.net), (x.metric, x.type1));
            if x.tag != 0 {
                st.external_tags.insert((*d, x.net), x.tag);
            }
        }
    }
    for d in &originators {
        st.externals.entry(*d).or_default().push(Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0));
    }
    let areas_of: BTreeMap<DeviceId, BTreeSet<u32>> =
        routers.iter().map(|d| (d.id, ifs.get(&d.id).into_iter().flatten().map(|(i, _)| area_of(d, i)).collect())).collect();
    // Border routers of an area: in it and in another.
    let abrs = |area: u32| -> Vec<DeviceId> { areas_of.iter().filter(|(_, a)| a.len() > 1 && a.contains(&area)).map(|(d, _)| *d).collect() };
    // An area whose routes reach the backbone (and so every other area): the backbone itself, or
    // one with a border router attached to it (by an interface or a virtual link).
    let reaches_backbone = |area: u32| area == 0 || abrs(area).iter().any(|b| backbone.contains(b));
    for area in areas_of.values().flatten().copied().collect::<BTreeSet<u32>>() {
        if let Some(tr) = translator_of(t, &areas_of, &st.router_ids, area) {
            st.translators.insert(area, tr);
        }
    }
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
        // A router wholly inside a stub area or an NSSA hears no type 5 external routes; with
        // no-summary on its border router, no routes from other areas either. It can hear other
        // areas only when its area (some border router of it) is joined to the backbone.
        let closed: Option<(u32, AreaKind)> = my_areas
            .first()
            .copied()
            .filter(|_| my_areas.len() == 1)
            .map(|a| (a, t.device(r.id).map_or(AreaKind::Normal, |x| area_kind(x, a))))
            .filter(|(_, k)| *k != AreaKind::Normal);
        let totally = closed.is_some_and(|(a, _)| abrs(a).iter().any(|b| conf(*b, a).no_summary));
        let hears_others = my_areas.iter().any(|a| reaches_backbone(*a));
        // How this router hears the external routes router `x` redistributes: as type 5 (false),
        // as type 7 inside an NSSA (true), or not at all.
        let heard = |x: DeviceId| -> Option<bool> {
            let x_areas = areas_of.get(&x)?;
            let xo = t.device(x).and_then(|d| d.ospf.as_ref())?;
            // The router must be reachable: in an area this one is in, or in one that reaches the
            // backbone when this router's own areas do (an area cut off from the backbone knows
            // of no AS boundary router outside it, so it hears no external route from there).
            let shared = x_areas.iter().any(|a| my_areas.contains(a));
            let through_backbone = hears_others && x_areas.iter().any(|a| reaches_backbone(*a));
            if !(shared || through_backbone) {
                return None;
            }
            // A border router with `no-redistribution` keeps its own routes out of the NSSA.
            let outside = x_areas.iter().any(|a| kind_of(xo, *a) != AreaKind::Nssa);
            let x_nssa: Vec<u32> =
                x_areas.iter().copied().filter(|a| kind_of(xo, *a) == AreaKind::Nssa && !(outside && xo.area(*a).nssa_no_redist)).collect();
            match closed {
                Some((_, AreaKind::Stub)) => None,
                Some((a, _)) => x_nssa.contains(&a).then_some(true),
                None if my_areas.iter().any(|a| x_nssa.contains(a)) => Some(true),
                // A router only in NSSAs hands its routes to the translator, which sends them on as type 5.
                None if !x_nssa.is_empty() && x_areas.iter().all(|a| kind_of(xo, *a) == AreaKind::Nssa) => {
                    x_nssa.iter().any(|a| st.translators.contains_key(a)).then_some(false)
                }
                None => Some(false),
            }
        };
        let mut routes: BTreeMap<Ipv4Net, OspfRoute> = BTreeMap::new();
        // Other paths of the same cost to a destination, beside the one in `routes`.
        let mut more: BTreeMap<Ipv4Net, Vec<OspfRoute>> = BTreeMap::new();
        // Intra-area routes beat inter-area ones, those beat external type 1, then type 2.
        let rank = |x: &OspfRoute| -> u8 {
            if !x.external {
                u8::from(x.inter_area)
            } else if x.type1 {
                2
            } else {
                3
            }
        };
        let mut add = |routes: &mut BTreeMap<Ipv4Net, OspfRoute>, cand: OspfRoute| {
            // `distribute-list in`: what this router will not install.
            if !netlabcraft_model::redist::passes(r, dist_of(r), true, Some(cand.iface.as_str()), cand.net, cand.cost) {
                return;
            }
            match routes.get(&cand.net) {
                Some(old) if rank(old) == rank(&cand) && old.cost == cand.cost => {
                    let list = more.entry(cand.net).or_default();
                    if old.via != cand.via && !list.iter().any(|x| x.via == cand.via) {
                        list.push(cand);
                    }
                }
                Some(old) if (rank(old), old.cost, old.via) <= (rank(&cand), cand.cost, cand.via) => {}
                _ => {
                    more.remove(&cand.net);
                    routes.insert(cand.net, cand);
                }
            }
        };
        for (x, (cx, hops)) in &best {
            for (ifc, via) in hops {
                for (net, c, area) in stubs.get(x).into_iter().flatten() {
                    if mine.contains(net) {
                        continue;
                    }
                    let inter_area = !my_areas.contains(area);
                    if inter_area && (totally || !hears_others || !reaches_backbone(*area)) {
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
                    let cand = OspfRoute {
                        net: *net,
                        via: *via,
                        iface: ifc.name.clone(),
                        cost: cx.saturating_add(*c),
                        external: false,
                        type1: false,
                        inter_area,
                        nssa: false,
                        tag: 0,
                    };
                    add(&mut routes, cand);
                }
                let how = if *x == r.id { None } else { heard(*x) };
                for sd in redistributed.get(x).into_iter().flatten().filter(|_| how.is_some()) {
                    if mine.contains(&sd.net) {
                        continue;
                    }
                    // Type 2 keeps the metric it was given whatever the path; type 1 adds the way to the router.
                    let cost = if sd.type1 { cx.saturating_add(sd.metric) } else { sd.metric };
                    let cand = OspfRoute {
                        net: sd.net,
                        via: *via,
                        iface: ifc.name.clone(),
                        cost,
                        external: true,
                        type1: sd.type1,
                        inter_area: false,
                        nssa: how == Some(true),
                        tag: sd.tag,
                    };
                    add(&mut routes, cand);
                }
                if originators.contains(x) && how.is_some() {
                    let def = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0);
                    let cand = OspfRoute {
                        net: def,
                        via: *via,
                        iface: ifc.name.clone(),
                        cost: 1,
                        external: true,
                        type1: false,
                        inter_area: false,
                        nssa: how == Some(true),
                        tag: 0,
                    };
                    if routes.get(&def).is_none_or(|old| (*cx, cand.via) < (old.cost.saturating_add(*cx), old.via)) {
                        routes.insert(def, cand);
                    }
                }
            }
        }
        // The border routers of a stub area send a default route in its place; those of an NSSA
        // do when asked to (`default-information-originate`, a type 7 default) or when the area is
        // totally stubby (`no-summary`, a type 3 default).
        if let Some((a, kind)) = closed {
            let def = Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0);
            for b in abrs(a) {
                let bc = conf(b, a);
                let type3 = kind == AreaKind::Stub || bc.no_summary;
                if !type3 && !bc.nssa_default {
                    continue;
                }
                if let Some((cx, hops)) = best.get(&b)
                    && let Some((ifc, via)) = hops.first()
                {
                    let cand = if type3 {
                        OspfRoute {
                            net: def,
                            via: *via,
                            iface: ifc.name.clone(),
                            cost: cx.saturating_add(bc.default_cost.unwrap_or(1)),
                            external: false,
                            type1: false,
                            inter_area: true,
                            nssa: false,
                            tag: 0,
                        }
                    } else {
                        // A type 7 default is an external type 2 route with metric 1.
                        OspfRoute {
                            net: def,
                            via: *via,
                            iface: ifc.name.clone(),
                            cost: 1,
                            external: true,
                            type1: false,
                            inter_area: false,
                            nssa: true,
                            tag: 0,
                        }
                    };
                    let rank_cost = |x: &OspfRoute| (x.cost, x.via);
                    if routes.get(&def).is_none_or(|old| rank_cost(&cand) < rank_cost(old)) {
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
            format!(
                "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{}{:?}",
                d.ospf, d.rip, d.eigrp, d.routes, d.access.users, d.ospf_if, d.eigrp_if, d.ip_routing, d.bgp
            )
            .hash(&mut h);
            if !d.route_maps.is_empty() || !d.prefix_lists.is_empty() {
                // Policy decides what is redistributed and what BGP accepts, so edits count.
                format!("{:?}{:?}{:?}", d.prefix_lists, d.route_maps, d.acls).hash(&mut h);
            }
            if !d.community_lists.is_empty() || !d.as_path_lists.is_empty() {
                // Community lists and AS-path lists are what `match community` and `match as-path` read.
                format!("{:?}{:?}", d.community_lists, d.as_path_lists).hash(&mut h);
            }
            d.eigrp_summary.hash(&mut h);
            if !d.key_chains.is_default() {
                // Key chains and the clock they are read against decide which keys a router uses.
                format!("{:?}{:?}", d.key_chains, d.clock_unix).hash(&mut h);
            }
            d.tunnels.hash(&mut h);
            format!("{:?}", d.loopbacks).hash(&mut h);
            for s in &d.svis {
                (s.vlan, s.ip, s.shutdown).hash(&mut h);
            }
        }
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Vec<u8> {
        netlabcraft_wire::control::OspfHello {
            options: OSPF_OPT_E,
            router_id: Ipv4Addr::new(1, 1, 1, 1),
            area: 0,
            mask: Ipv4Addr::UNSPECIFIED,
            hello_secs: 10,
            dead_secs: 40,
            priority: 1,
            dr: Ipv4Addr::UNSPECIFIED,
            bdr: Ipv4Addr::UNSPECIFIED,
            neighbors: Vec::new(),
        }
        .encode()
    }

    fn signed(seq: u32) -> Vec<u8> {
        netlabcraft_wire::control::ospf_authenticate(hello(), &OspfAuthSend::Md5 { key_id: 1, key: "sk-test", seq })
    }

    #[test]
    fn a_sequence_number_below_the_last_one_heard_is_a_replay() {
        assert!(replay_problem(Some(5), &signed(4)).is_some_and(|w| w.contains("replayed")));
        assert!(replay_problem(Some(5), &signed(5)).is_none(), "several packets leave in one second");
        assert!(replay_problem(Some(5), &signed(6)).is_none());
        assert!(replay_problem(None, &signed(1)).is_none(), "nothing heard yet");
        let plain = netlabcraft_wire::control::ospf_authenticate(hello(), &OspfAuthSend::Null);
        assert!(replay_problem(Some(5), &plain).is_none());
        assert!(replay_problem(Some(5), &[1, 2, 3]).is_none(), "a truncated packet is not a replay");
    }
}
