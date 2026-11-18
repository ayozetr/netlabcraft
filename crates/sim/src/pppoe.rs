//! PPPoE (RFC 2516) on the wire: a PC's PPPoE Dialer, a router's dialer interface, and a server's
//! PPPoE service or a router's `bba-group` as an access concentrator.
//!
//! Everything crosses the cables as real frames, so it passes through switches and shows in the
//! event list and in captures. A dialer broadcasts a PADI, a concentrator answers with a PADO
//! (its name and a cookie), the dialer asks for a session with a PADR and gets a session id in
//! the PADS. PPP then runs inside session frames: LCP agrees on the link (the MRU is 1,492 and
//! the concentrator asks for PAP or CHAP), PAP or CHAP (a real MD5 challenge) authenticates the
//! caller, and IPCP gives it an address (the caller asks for 0.0.0.0, the concentrator refuses
//! with the address from its pool, the caller asks for that one) and tells it the concentrator's
//! own address. After that every datagram that goes to the concentrator, or from it to the
//! caller, is a PPP IPv4 packet in a session frame; a PADT ends the session.
//!
//! **State.** While two ends negotiate, the concentrator keeps a [`Neg`] per session id (in the
//! device's dynamic state, bounded). An established session is not kept in the simulator at all:
//! the lab holds it (`Device::pppoe::session` on the caller, with its session id and the
//! concentrator's MAC), and the concentrator recognises a session by finding that record, so a
//! saved lab, an undo and a rebuilt simulator all keep sessions working. A concentrator that is
//! switched off, or whose service is off, no longer recognises its sessions and answers their
//! frames with a PADT.
//!
//! **Routers.** A router is a caller through its dialer interfaces (the session is kept in
//! `Device::pppoe_router`, with the Ethernet port it runs over) and a concentrator on every port
//! that enabled PPPoE with a `bba-group` whose virtual template gives an address. The routing
//! side is derived from the same records, so nothing has to be cleaned up when a session ends:
//! the caller's `Dialer` interface exists while its session does, and the concentrator has a
//! host route (`/32`) for every address a session holds, out of its `Virtual-Access` interface
//! (see [`router_ifaces`] and [`host_routes`]).
//!
//! What the simulation leaves out is in `docs/gaps.md` (no retransmission timers, one fixed set
//! of LCP options).

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net, MacAddr};
use netlabcraft_model::pppoe::{MAX_SESSIONS, PppoeSession, VirtualTemplate};
use netlabcraft_model::{Device, DeviceKind, End, PppAuth, Topology};
use netlabcraft_wire::Frame;
use netlabcraft_wire::md5::md5;
use netlabcraft_wire::pppoe::{self, Control, Discovery, Opt, SessionPacket, Tag};

use crate::{L3If, SVI_PORT, Sim, SimError, explain, l3_ifaces, port_up};

/// How long a dialer waits for an answer before it asks again or gives up.
const WAIT_US: u64 = 2_000_000;
/// How long a keep-alive probe waits.
const PROBE_US: u64 = 1_000_000;
/// Negotiations a concentrator keeps at once, and how long one may sit unfinished.
const MAX_NEG: usize = 32;
const NEG_AGE_US: u64 = 30_000_000;
/// Packets a dialer holds while it waits.
const MAX_HEARD: usize = 32;

/// What a device keeps about PPPoE.
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    /// A dialer is waiting for answers: what it hears goes to `inbox`.
    listening: bool,
    inbox: Vec<Heard>,
    /// A concentrator's negotiations under way, by session id.
    neg: BTreeMap<u16, Neg>,
}

#[derive(Debug, Clone)]
struct Heard {
    src: MacAddr,
    what: What,
}

#[derive(Debug, Clone)]
enum What {
    Discovery(Discovery),
    Ppp { session: u16, protocol: u16, packet: Control },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Lcp,
    Auth,
    Ipcp,
}

/// One session being set up at a concentrator.
#[derive(Debug, Clone)]
struct Neg {
    client: MacAddr,
    stage: Stage,
    /// The caller acknowledged our LCP request / we acknowledged its.
    ours_acked: bool,
    theirs_acked: bool,
    challenge: Vec<u8>,
    chap_id: u8,
    /// The address this caller will get.
    address: Ipv4Addr,
    ip_ours_acked: bool,
    ip_theirs_acked: bool,
    since: u64,
}

/// What a dialer learned from a concentrator.
#[derive(Debug, Clone)]
pub struct DialResult {
    pub session: u16,
    pub server_mac: MacAddr,
    pub ac_name: String,
    pub address: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub auth: PppAuth,
}

/// A concentrator's answer to a PADI.
struct Offer {
    ac: MacAddr,
    name: String,
    cookie: Option<Vec<u8>>,
    service: Vec<u8>,
}

fn can_dial(kind: DeviceKind) -> bool {
    matches!(kind, DeviceKind::Pc | DeviceKind::Laptop | DeviceKind::Server | DeviceKind::Router)
}

/// What a concentrator offers on one of its ports.
struct Concentrator {
    /// What a caller must prove: `None` is nothing (a router's template without `ppp authentication`).
    auth: Option<PppAuth>,
    /// The concentrator's own address on the session: the caller's default gateway.
    me: Option<Ipv4Addr>,
    /// A server hands out only addresses of its port's subnet; a router's pool is its own.
    subnet: Option<Ipv4Net>,
    /// The addresses of the pool, in order.
    pool: Vec<Ipv4Addr>,
    /// The largest IP packet of a session.
    mtu: u16,
    /// Why there is no address to give, when `me` is none.
    no_address: &'static str,
}

/// The address a virtual template gives its sessions as the concentrator's own: its `ip address`,
/// else the address of the interface it is unnumbered to.
pub fn template_address(d: &Device, t: &VirtualTemplate) -> Option<Ipv4Addr> {
    if let Some(ip) = t.ip {
        return Some(ip.addr);
    }
    let name = t.unnumbered.as_deref()?;
    d.ports
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .and_then(|p| p.ip)
        .or_else(|| d.loopbacks.iter().find(|l| l.name().eq_ignore_ascii_case(name)).and_then(|l| l.ip))
        .map(|n| n.addr)
}

/// What the device answers PPPoE with on `port`, if it is an access concentrator there: a server
/// with the service on, or a router whose port enabled PPPoE with a group that has a template.
fn concentrator(d: &Device, port: usize) -> Option<Concentrator> {
    if d.powered_off {
        return None;
    }
    match d.kind {
        DeviceKind::Server if d.services.pppoe.on => {
            let cfg = &d.services.pppoe;
            let net = d.ports.get(port).and_then(|p| p.ip);
            Some(Concentrator {
                auth: Some(cfg.auth),
                me: net.map(|n| n.addr),
                subnet: net,
                pool: cfg.pool().collect(),
                mtu: pppoe::MRU,
                no_address: "the concentrator has no IP address on this port",
            })
        }
        DeviceKind::Router => {
            let p = d.ports.get(port)?;
            let tpl = d.pppoe_router.template_of_port(&p.name)?;
            let pool: Vec<Ipv4Addr> =
                tpl.pool.as_deref().and_then(|n| d.crypto.ra.pools.iter().find(|x| x.name == n)).map(|x| x.addresses().collect()).unwrap_or_default();
            Some(Concentrator {
                auth: tpl.auth,
                me: template_address(d, tpl),
                subnet: None,
                pool,
                mtu: tpl.mtu.unwrap_or(pppoe::MRU).min(pppoe::MRU),
                no_address: "the virtual template has no address (ip address or ip unnumbered)",
            })
        }
        _ => None,
    }
}

/// Whether the device answers PPP calls on this port: a server with the service on, or a router
/// whose port enabled it with a group that has a template (a modem line's `modem inout`).
pub fn answers(d: &Device, port: usize) -> bool {
    concentrator(d, port).is_some()
}

/// Whether a caller's PAP request is right for this concentrator's accounts.
fn pap_ok(d: &Device, user: &str, password: &str) -> bool {
    if d.kind == DeviceKind::Server {
        d.services.pppoe.user(user).is_some_and(|x| x.password == password)
    } else {
        d.access.users.iter().any(|u| u.name == user && u.matches(password))
    }
}

/// The shared secret of the account `user`, when it can be used to check a CHAP response. A
/// router keeps `username NAME secret …` only as a hash, which cannot answer a challenge.
fn chap_secret(d: &Device, user: &str) -> Option<String> {
    if d.kind == DeviceKind::Server {
        d.services.pppoe.user(user).map(|x| x.password.clone())
    } else {
        d.access.users.iter().find(|u| u.name == user && !u.hashed).map(|u| u.credential.clone())
    }
}

/// The cookie a concentrator hands one caller in its PADO and expects back in the PADR.
fn cookie(d: &Device, client: MacAddr) -> [u8; 8] {
    let mut m = client.0.to_vec();
    m.extend_from_slice(&d.id.0.to_be_bytes());
    let h = md5(&m);
    [h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]]
}

/// A device's LCP magic number: never zero, different between devices and callers.
fn magic_of(d: &Device, peer: MacAddr) -> u32 {
    let mut m = peer.0.to_vec();
    m.extend_from_slice(&d.id.0.to_be_bytes());
    let h = md5(&m);
    u32::from_be_bytes([h[0], h[1], h[2], h[3]]) | 1
}

/// The session a device holds as a caller on `port`: a computer's on its first port, a router's
/// on the Ethernet port its dialer runs over.
fn client_session(d: &Device, port: usize) -> Option<&PppoeSession> {
    if d.kind == DeviceKind::Router {
        let name = &d.ports.get(port)?.name;
        d.pppoe_router.dialers.iter().find_map(|x| x.session.as_ref().filter(|s| &s.port == name).map(|s| &s.session))
    } else if port == 0 {
        d.pppoe.session.as_ref()
    } else {
        None
    }
}

/// Every session the lab holds: the device that dialed it, the port it dialed from, the record.
fn calls(t: &Topology) -> impl Iterator<Item = (&Device, usize, &PppoeSession)> + '_ {
    t.devices.iter().flat_map(|c| {
        let host = c.pppoe.session.as_ref().filter(|_| !c.ports.is_empty()).map(|s| (c, 0usize, s));
        let routed = c.pppoe_router.dialers.iter().filter_map(move |x| {
            let s = x.session.as_ref()?;
            let port = c.ports.iter().position(|p| p.name == s.port)?;
            Some((c, port, &s.session))
        });
        host.into_iter().chain(routed)
    })
}

/// Whether this concentrator holds a session with this id for the caller with this MAC: the
/// caller's lab record names this device and the port that still answers.
fn has_session(t: &Topology, d: &Device, sid: u16, client: MacAddr) -> bool {
    sid != 0
        && calls(t).any(|(c, cport, s)| {
            s.id == sid
                && s.server == d.id
                && c.ports.get(cport).is_some_and(|p| p.mac == client)
                && d.ports.iter().position(|p| p.mac == s.server_mac).is_some_and(|i| concentrator(d, i).is_some())
        })
}

/// Where a datagram goes as PPPoE: the peer's MAC, the session, the port it leaves from and the
/// largest packet it carries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Peer {
    mac: MacAddr,
    session: u16,
    port: usize,
    pub(crate) mtu: u16,
}

/// Where a datagram to next hop `nh` out of `out` goes as PPPoE, if it does. A caller sends what
/// is for its concentrator (a computer: its gateway; a router: whatever leaves by its dialer
/// interface); a concentrator sends what is for a caller's address.
fn peer(t: &Topology, d: &Device, out: &L3If, nh: Ipv4Addr) -> Option<Peer> {
    if out.vlan.is_some() || out.port == SVI_PORT {
        return None;
    }
    if d.kind == DeviceKind::Router {
        for dl in &d.pppoe_router.dialers {
            if let Some(ds) = dl.session.as_ref()
                && ds.session.id != 0
                && out.name == dl.name()
                && let Some(port) = d.ports.iter().position(|p| p.name == ds.port)
            {
                return Some(Peer { mac: ds.session.server_mac, session: ds.session.id, port, mtu: dl.mtu.unwrap_or(pppoe::MRU).min(pppoe::MRU) });
            }
        }
    } else if let Some(s) = d.pppoe.session.as_ref()
        && s.id != 0
        && out.port == 0
        && s.gateway == nh
    {
        return Some(Peer { mac: s.server_mac, session: s.id, port: 0, mtu: pppoe::MRU });
    }
    calls(t).find_map(|(c, cport, s)| {
        if s.id == 0 || s.server != d.id || s.address != nh {
            return None;
        }
        let sport = d.ports.iter().position(|p| p.mac == s.server_mac)?;
        let conc = concentrator(d, sport)?;
        Some(Peer { mac: c.ports.get(cport)?.mac, session: s.id, port: sport, mtu: conc.mtu })
    })
}

/// The layer-3 interfaces a router has because of PPPoE: a `Dialer` interface while its session
/// holds, and a `Virtual-Access` interface (the template's address, a `/32`) on every port that
/// answers PPPoE.
pub(crate) fn router_ifaces(d: &Device) -> Vec<L3If> {
    let mut out = Vec::new();
    if d.kind != DeviceKind::Router || !d.pppoe_router.active() {
        return out;
    }
    for dl in &d.pppoe_router.dialers {
        let Some(ds) = dl.session.as_ref() else { continue };
        if !dl.negotiated || dl.shutdown {
            continue;
        }
        let Some((i, p)) = d.ports.iter().enumerate().find(|(_, p)| p.name == ds.port) else { continue };
        out.push(L3If { port: i, sub: None, vlan: None, net: Ipv4Net::new(ds.session.address, 32), mac: p.mac, name: dl.name(), vrf: None });
    }
    let mut n = 0;
    for (i, p) in d.ports.iter().enumerate() {
        if p.shutdown {
            continue;
        }
        let Some(addr) = d.pppoe_router.template_of_port(&p.name).and_then(|t| template_address(d, t)) else { continue };
        n += 1;
        out.push(L3If { port: i, sub: None, vlan: None, net: Ipv4Net::new(addr, 32), mac: p.mac, name: format!("Virtual-Access{n}"), vrf: None });
    }
    out
}

/// The host routes a router has because of PPPoE: its concentrator's address on a session of
/// the caller's (`Virtual-Access`), and the concentrator's address on its dialer's session
/// (`Dialer`). Each is a `/32` and the name of the interface it leaves by (one of `ifs`).
pub(crate) fn host_routes(t: &Topology, d: &Device, ifs: &[L3If]) -> Vec<(Ipv4Net, String)> {
    let mut out = Vec::new();
    if d.kind != DeviceKind::Router || !d.pppoe_router.active() {
        return out;
    }
    for dl in &d.pppoe_router.dialers {
        if let Some(ds) = dl.session.as_ref()
            && ifs.iter().any(|i| i.name == dl.name())
        {
            out.push((Ipv4Net::new(ds.session.gateway, 32), dl.name()));
        }
    }
    for (_, _, s) in calls(t).filter(|(_, _, s)| s.server == d.id && s.id != 0) {
        let Some(sport) = d.ports.iter().position(|p| p.mac == s.server_mac) else { continue };
        if concentrator(d, sport).is_none() {
            continue;
        }
        if let Some(i) = ifs.iter().find(|i| i.port == sport && i.name.starts_with("Virtual-Access")) {
            out.push((Ipv4Net::new(s.address, 32), i.name.clone()));
        }
    }
    out.truncate(MAX_SESSIONS + netlabcraft_model::pppoe::MAX_GROUPS);
    out
}

/// A session a concentrator holds.
#[derive(Debug, Clone)]
pub struct Served {
    pub session: u16,
    /// The concentrator's port it runs over.
    pub port: usize,
    pub client: DeviceId,
    pub client_mac: MacAddr,
    pub address: Ipv4Addr,
    /// The interface the session is on from the concentrator's side (`Virtual-Access1`, or the port).
    pub interface: String,
}

/// The sessions the device holds as a concentrator, in the lab's order.
pub fn served(t: &Topology, server: DeviceId) -> Vec<Served> {
    let Some(d) = t.device(server) else { return Vec::new() };
    let ifs = l3_ifaces(d);
    calls(t)
        .filter(|(_, _, s)| s.server == server && s.id != 0)
        .filter_map(|(c, cport, s)| {
            let port = d.ports.iter().position(|p| p.mac == s.server_mac)?;
            concentrator(d, port)?;
            let interface = ifs
                .iter()
                .find(|i| i.port == port && i.name.starts_with("Virtual-Access"))
                .map_or_else(|| d.ports.get(port).map(|p| p.name.clone()).unwrap_or_default(), |i| i.name.clone());
            Some(Served { session: s.id, port, client: c.id, client_mac: c.ports.get(cport)?.mac, address: s.address, interface })
        })
        .take(MAX_SESSIONS)
        .collect()
}

/// Every IPv4 address some device of the lab holds.
fn used_addresses(t: &Topology) -> BTreeSet<Ipv4Addr> {
    let mut used = BTreeSet::new();
    for d in &t.devices {
        used.extend(l3_ifaces(d).iter().map(|i| i.net.addr));
        used.extend(d.ports.iter().filter_map(|p| p.ip).map(|n| n.addr));
    }
    used
}

/// What a dialer proves itself with: a name and secret for CHAP, and for PAP.
#[derive(Debug, Clone)]
pub struct DialCreds {
    pub chap_name: String,
    pub chap_secret: String,
    pub pap_name: String,
    pub pap_secret: String,
}

impl DialCreds {
    /// The same account for both (a computer's PPPoE Dialer).
    pub fn same(user: &str, password: &str) -> DialCreds {
        DialCreds { chap_name: user.to_string(), chap_secret: password.to_string(), pap_name: user.to_string(), pap_secret: password.to_string() }
    }
}

impl Sim {
    // ---- the data path ----

    /// The IP MTU of a PPPoE session is the PPP MRU, 1,492: asks whether `nh` is reached that way.
    pub(crate) fn pppoe_peer(&self, t: &Topology, d: &Device, out: &L3If, nh: Ipv4Addr) -> Option<Peer> {
        peer(t, d, out, nh)
    }

    /// Sends a datagram as a PPP IPv4 packet in a session frame when its next hop is a session
    /// peer. Returns whether it did.
    pub(crate) fn pppoe_tx(&mut self, t: &Topology, d: &Device, out: &L3If, nh: Ipv4Addr, datagram: &[u8]) -> bool {
        let Some(p) = peer(t, d, out, nh) else { return false };
        let payload = SessionPacket { session_id: p.session, protocol: pppoe::PPP_IP, data: datagram.to_vec() }.encode();
        self.pppoe_emit(t, d, p.port, p.mac, pppoe::ETHERTYPE_SESSION, payload);
        true
    }

    // ---- sending ----

    fn pppoe_emit(&mut self, t: &Topology, d: &Device, port: usize, dst: MacAddr, ethertype: u16, payload: Vec<u8>) {
        let Some(p) = d.ports.get(port) else { return };
        let frame = Frame { dst, src: p.mac, vlan: None, ethertype, payload };
        self.emit_frame(t, d, port, &frame);
    }

    #[allow(clippy::too_many_arguments)]
    fn pppoe_ppp(&mut self, t: &Topology, d: &Device, port: usize, dst: MacAddr, session_id: u16, protocol: u16, c: Control) {
        let payload = SessionPacket { session_id, protocol, data: c.encode() }.encode();
        self.pppoe_emit(t, d, port, dst, pppoe::ETHERTYPE_SESSION, payload);
    }

    fn pppoe_padt(&mut self, t: &Topology, d: &Device, port: usize, dst: MacAddr, session_id: u16) {
        let padt = Discovery { code: pppoe::CODE_PADT, session_id, tags: Vec::new() };
        self.pppoe_emit(t, d, port, dst, pppoe::ETHERTYPE_DISCOVERY, padt.encode());
    }

    // ---- receiving ----

    /// A PPPoE frame arrives at a host.
    pub(crate) fn pppoe_in(&mut self, t: &Topology, d: &Device, in_port: usize, f: &Frame) -> Result<(), String> {
        let port = d.ports.get(in_port).ok_or("port removed")?;
        if f.dst != port.mac && !f.dst.is_broadcast() {
            return Err("not addressed to this device".into());
        }
        if !can_dial(d.kind) {
            return Err("this device does not speak PPPoE".into());
        }
        if f.ethertype == pppoe::ETHERTYPE_DISCOVERY { self.pppoe_discovery_in(t, d, in_port, f) } else { self.pppoe_session_in(t, d, in_port, f) }
    }

    fn pppoe_hear(&mut self, d: &Device, src: MacAddr, what: What) {
        let st = self.st(d.id);
        if st.pppoe.listening && st.pppoe.inbox.len() < MAX_HEARD {
            st.pppoe.inbox.push(Heard { src, what });
        }
    }

    fn pppoe_discovery_in(&mut self, t: &Topology, d: &Device, in_port: usize, f: &Frame) -> Result<(), String> {
        let msg = Discovery::decode(&f.payload).map_err(|e| e.to_string())?;
        match msg.code {
            pppoe::CODE_PADI if concentrator(d, in_port).is_some() => {
                let mut tags =
                    vec![Tag::new(pppoe::TAG_SERVICE_NAME, msg.tag(pppoe::TAG_SERVICE_NAME).unwrap_or(&[])), Tag::text(pppoe::TAG_AC_NAME, &d.name)];
                if let Some(u) = msg.tag(pppoe::TAG_HOST_UNIQ) {
                    tags.push(Tag::new(pppoe::TAG_HOST_UNIQ, u));
                }
                tags.push(Tag::new(pppoe::TAG_AC_COOKIE, &cookie(d, f.src)));
                let pado = Discovery { code: pppoe::CODE_PADO, session_id: 0, tags };
                self.pppoe_emit(t, d, in_port, f.src, pppoe::ETHERTYPE_DISCOVERY, pado.encode());
                Ok(())
            }
            pppoe::CODE_PADR if concentrator(d, in_port).is_some() => {
                if f.dst.is_broadcast() {
                    return Err("a PADR is sent to one concentrator".into());
                }
                if msg.tag(pppoe::TAG_AC_COOKIE) != Some(&cookie(d, f.src)[..]) {
                    return Err("PADR: the AC-Cookie is not the one that was offered".into());
                }
                self.pppoe_session_request(t, d, in_port, f.src, &msg);
                Ok(())
            }
            pppoe::CODE_PADT => {
                self.st(d.id).pppoe.neg.retain(|id, n| !(*id == msg.session_id && n.client == f.src));
                if let Some(s) = client_session(d, in_port)
                    && s.id != 0
                    && s.id == msg.session_id
                    && s.server_mac == f.src
                {
                    self.pppoe_ended.push((d.id, s.id));
                }
                self.pppoe_hear(d, f.src, What::Discovery(msg));
                Ok(())
            }
            pppoe::CODE_PADO | pppoe::CODE_PADS => {
                self.pppoe_hear(d, f.src, What::Discovery(msg));
                Ok(())
            }
            _ => Err("no PPPoE service here".into()),
        }
    }

    /// A PADR: the concentrator picks the caller's address and a session id, answers with a
    /// PADS and starts LCP, or refuses with a PADS that carries the reason.
    fn pppoe_session_request(&mut self, t: &Topology, d: &Device, in_port: usize, client: MacAddr, msg: &Discovery) {
        let mut tags = vec![Tag::new(pppoe::TAG_SERVICE_NAME, msg.tag(pppoe::TAG_SERVICE_NAME).unwrap_or(&[]))];
        if let Some(u) = msg.tag(pppoe::TAG_HOST_UNIQ) {
            tags.push(Tag::new(pppoe::TAG_HOST_UNIQ, u));
        }
        let now = self.now;
        // A caller that asks again starts over: what was left of its last try is dropped.
        self.st(d.id).pppoe.neg.retain(|_, n| n.client != client);
        self.st(d.id).pppoe.neg.retain(|_, n| now.saturating_sub(n.since) < NEG_AGE_US);
        let refuse = |me: &mut Sim, mut tags: Vec<Tag>, why: &str| {
            tags.push(Tag::text(pppoe::TAG_AC_SYSTEM_ERROR, why));
            let pads = Discovery { code: pppoe::CODE_PADS, session_id: 0, tags };
            me.pppoe_emit(t, d, in_port, client, pppoe::ETHERTYPE_DISCOVERY, pads.encode());
        };
        let Some(conc) = concentrator(d, in_port) else { return };
        if conc.me.is_none() {
            refuse(&mut *self, tags, conc.no_address);
            return;
        }
        if calls(t).filter(|(_, _, x)| x.server == d.id).count() >= MAX_SESSIONS {
            refuse(&mut *self, tags, "too many sessions are open");
            return;
        }
        let mut used = used_addresses(t);
        used.extend(self.st(d.id).pppoe.neg.values().map(|n| n.address));
        let Some(address) =
            conc.pool.iter().copied().find(|a| {
                conc.subnet.is_none_or(|net| net.contains(*a) && !Ipv4Net::new(*a, net.prefix).is_network_or_broadcast()) && !used.contains(a)
            })
        else {
            refuse(
                &mut *self,
                tags,
                if conc.subnet.is_some() { "no free address in the pool on this subnet" } else { "no free address in the pool" },
            );
            return;
        };
        let taken: BTreeSet<u16> = self.st(d.id).pppoe.neg.keys().copied().collect();
        let Some(session_id) = (1..u16::MAX).find(|i| !taken.contains(i) && !calls(t).any(|(_, _, x)| x.server == d.id && x.id == *i)) else {
            refuse(&mut *self, tags, "no session id is free");
            return;
        };
        if self.st(d.id).pppoe.neg.len() >= MAX_NEG {
            refuse(&mut *self, tags, "too many sessions are being set up");
            return;
        }
        let challenge = md5(format!("{}|{client}|{now}", d.name).as_bytes()).to_vec();
        let chap_id = challenge.first().copied().unwrap_or(1);
        let neg = Neg {
            client,
            stage: Stage::Lcp,
            ours_acked: false,
            theirs_acked: false,
            challenge,
            chap_id,
            address,
            ip_ours_acked: false,
            ip_theirs_acked: false,
            since: now,
        };
        self.st(d.id).pppoe.neg.insert(session_id, neg);
        let pads = Discovery { code: pppoe::CODE_PADS, session_id, tags };
        self.pppoe_emit(t, d, in_port, client, pppoe::ETHERTYPE_DISCOVERY, pads.encode());
        let mut opts = vec![pppoe::mru_option(conc.mtu)];
        if let Some(a) = conc.auth {
            opts.push(pppoe::auth_option(a == PppAuth::Chap));
        }
        opts.push(pppoe::magic_option(magic_of(d, client)));
        self.pppoe_ppp(t, d, in_port, client, session_id, pppoe::PPP_LCP, Control::new(pppoe::CONF_REQ, 1, pppoe::encode_options(&opts)));
    }

    fn pppoe_session_in(&mut self, t: &Topology, d: &Device, in_port: usize, f: &Frame) -> Result<(), String> {
        let sp = SessionPacket::decode(&f.payload).map_err(|e| e.to_string())?;
        if concentrator(d, in_port).is_some() {
            if sp.protocol == pppoe::PPP_IP {
                if !has_session(t, d, sp.session_id, f.src) {
                    self.pppoe_padt(t, d, in_port, f.src, sp.session_id);
                    return Err("no such PPPoE session".into());
                }
                return self.pppoe_ip_in(t, d, in_port, &sp.data);
            }
            return self.pppoe_server_ppp(t, d, in_port, f.src, &sp);
        }
        let live = client_session(d, in_port).is_some_and(|s| s.id != 0 && s.id == sp.session_id && s.server_mac == f.src);
        if sp.protocol == pppoe::PPP_IP {
            if !live {
                return Err("not one of this device's PPPoE sessions".into());
            }
            return self.pppoe_ip_in(t, d, in_port, &sp.data);
        }
        let c = Control::decode(&sp.data).map_err(|e| e.to_string())?;
        if self.st(d.id).pppoe.listening {
            self.pppoe_hear(d, f.src, What::Ppp { session: sp.session_id, protocol: sp.protocol, packet: c });
            return Ok(());
        }
        // A session that is up answers the concentrator's keep-alives.
        if live && sp.protocol == pppoe::PPP_LCP {
            match c.code {
                pppoe::ECHO_REQ => {
                    let magic = magic_of(d, f.src).to_be_bytes().to_vec();
                    self.pppoe_ppp(t, d, in_port, f.src, sp.session_id, pppoe::PPP_LCP, Control::new(pppoe::ECHO_REP, c.id, magic));
                }
                pppoe::TERM_REQ => {
                    self.pppoe_ppp(t, d, in_port, f.src, sp.session_id, pppoe::PPP_LCP, Control::new(pppoe::TERM_ACK, c.id, Vec::new()));
                    self.pppoe_ended.push((d.id, sp.session_id));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn pppoe_ip_in(&mut self, t: &Topology, d: &Device, in_port: usize, data: &[u8]) -> Result<(), String> {
        // A router takes the packet on the interface the session is: its dialer or its virtual access.
        let ifc = l3_ifaces(d)
            .into_iter()
            .find(|i| {
                i.port == in_port
                    && i.vlan.is_none()
                    && (d.kind != DeviceKind::Router || i.name.starts_with("Dialer") || i.name.starts_with("Virtual-Access"))
            })
            .ok_or_else(|| "the interface has no IP address".to_string())?;
        self.note(explain::Step::ForMe { iface: ifc.name.clone() });
        self.ip_in(t, d, &ifc, data)
    }

    /// Ends a failed negotiation from the concentrator's side: LCP Terminate-Request, then PADT.
    fn pppoe_close(&mut self, t: &Topology, d: &Device, port: usize, client: MacAddr, session_id: u16) {
        self.pppoe_ppp(t, d, port, client, session_id, pppoe::PPP_LCP, Control::new(pppoe::TERM_REQ, 2, Vec::new()));
        self.pppoe_padt(t, d, port, client, session_id);
    }

    /// PPP packets a concentrator receives: the negotiation of a session being set up, or the
    /// keep-alives and the end of one that is up.
    fn pppoe_server_ppp(&mut self, t: &Topology, d: &Device, in_port: usize, client: MacAddr, sp: &SessionPacket) -> Result<(), String> {
        let sid = sp.session_id;
        let c = Control::decode(&sp.data).map_err(|e| e.to_string())?;
        let neg = self.st(d.id).pppoe.neg.get(&sid).filter(|n| n.client == client).cloned();
        let Some(mut n) = neg else {
            if !has_session(t, d, sid, client) {
                self.pppoe_padt(t, d, in_port, client, sid);
                return Err("no such PPPoE session".into());
            }
            if sp.protocol == pppoe::PPP_LCP {
                match c.code {
                    pppoe::ECHO_REQ => {
                        let magic = magic_of(d, client).to_be_bytes().to_vec();
                        self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_LCP, Control::new(pppoe::ECHO_REP, c.id, magic));
                    }
                    pppoe::TERM_REQ => {
                        self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_LCP, Control::new(pppoe::TERM_ACK, c.id, Vec::new()));
                    }
                    _ => {}
                }
            }
            return Ok(());
        };
        let Some(conc) = concentrator(d, in_port) else { return Ok(()) };
        let chap = conc.auth == Some(PppAuth::Chap);
        let pap = conc.auth == Some(PppAuth::Pap);
        let me = conc.me.unwrap_or(Ipv4Addr::UNSPECIFIED);
        let mut over = false;
        match (sp.protocol, c.code) {
            (pppoe::PPP_LCP, pppoe::CONF_REQ) => {
                if let Ok(opts) = pppoe::decode_options(&c.data) {
                    let unknown: Vec<Opt> = opts.iter().filter(|o| !matches!(o.kind, pppoe::LCP_MRU | pppoe::LCP_MAGIC)).cloned().collect();
                    if !unknown.is_empty() {
                        let rej = Control::new(pppoe::CONF_REJ, c.id, pppoe::encode_options(&unknown));
                        self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_LCP, rej);
                    } else if pppoe::opt_mru(&opts).is_some_and(|m| m > conc.mtu) {
                        let nak = Control::new(pppoe::CONF_NAK, c.id, pppoe::encode_options(&[pppoe::mru_option(conc.mtu)]));
                        self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_LCP, nak);
                    } else {
                        self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_LCP, Control::new(pppoe::CONF_ACK, c.id, c.data.clone()));
                        n.theirs_acked = true;
                    }
                }
            }
            (pppoe::PPP_LCP, pppoe::CONF_ACK) if c.id == 1 => n.ours_acked = true,
            (pppoe::PPP_LCP, pppoe::CONF_NAK | pppoe::CONF_REJ) => {
                self.pppoe_close(t, d, in_port, client, sid);
                over = true;
            }
            (pppoe::PPP_LCP, pppoe::TERM_REQ) => {
                self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_LCP, Control::new(pppoe::TERM_ACK, c.id, Vec::new()));
                over = true;
            }
            (pppoe::PPP_LCP, pppoe::ECHO_REQ) => {
                let magic = magic_of(d, client).to_be_bytes().to_vec();
                self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_LCP, Control::new(pppoe::ECHO_REP, c.id, magic));
            }
            (pppoe::PPP_PAP, pppoe::PAP_REQUEST) if n.stage == Stage::Auth && pap => {
                let ok = pppoe::parse_pap_request(&c.data).is_ok_and(|(u, p)| pap_ok(d, &u, &p));
                if ok {
                    self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_PAP, Control::new(pppoe::PAP_ACK, c.id, pppoe::message("Login ok")));
                    n.stage = Stage::Ipcp;
                    let req = Control::new(pppoe::CONF_REQ, 1, pppoe::encode_options(&[pppoe::address_option(me)]));
                    self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_IPCP, req);
                } else {
                    self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_PAP, Control::new(pppoe::PAP_NAK, c.id, pppoe::message("Login incorrect")));
                    self.pppoe_close(t, d, in_port, client, sid);
                    over = true;
                }
            }
            (pppoe::PPP_CHAP, pppoe::CHAP_RESPONSE) if n.stage == Stage::Auth && chap && c.id == n.chap_id => {
                let ok = pppoe::parse_chap_data(&c.data).is_ok_and(|(value, name)| {
                    chap_secret(d, &name).is_some_and(|secret| value.as_slice() == pppoe::chap_response(n.chap_id, &secret, &n.challenge).as_slice())
                });
                if ok {
                    self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_CHAP, Control::new(pppoe::CHAP_SUCCESS, c.id, b"Welcome".to_vec()));
                    n.stage = Stage::Ipcp;
                    let req = Control::new(pppoe::CONF_REQ, 1, pppoe::encode_options(&[pppoe::address_option(me)]));
                    self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_IPCP, req);
                } else {
                    self.pppoe_ppp(
                        t,
                        d,
                        in_port,
                        client,
                        sid,
                        pppoe::PPP_CHAP,
                        Control::new(pppoe::CHAP_FAILURE, c.id, b"Authentication failed".to_vec()),
                    );
                    self.pppoe_close(t, d, in_port, client, sid);
                    over = true;
                }
            }
            (pppoe::PPP_IPCP, pppoe::CONF_REQ) if n.stage == Stage::Ipcp => {
                let asked = pppoe::decode_options(&c.data).ok().and_then(|o| pppoe::opt_address(&o));
                if asked == Some(n.address) {
                    self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_IPCP, Control::new(pppoe::CONF_ACK, c.id, c.data.clone()));
                    n.ip_theirs_acked = true;
                } else {
                    let nak = Control::new(pppoe::CONF_NAK, c.id, pppoe::encode_options(&[pppoe::address_option(n.address)]));
                    self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_IPCP, nak);
                }
            }
            (pppoe::PPP_IPCP, pppoe::CONF_ACK) if n.stage == Stage::Ipcp && c.id == 1 => n.ip_ours_acked = true,
            _ => {}
        }
        if !over && n.stage == Stage::Lcp && n.ours_acked && n.theirs_acked {
            n.stage = Stage::Auth;
            if chap {
                let data = pppoe::chap_data(&n.challenge, &d.name);
                self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_CHAP, Control::new(pppoe::CHAP_CHALLENGE, n.chap_id, data));
            } else if conc.auth.is_none() {
                // Nothing to prove: straight to the address.
                n.stage = Stage::Ipcp;
                let req = Control::new(pppoe::CONF_REQ, 1, pppoe::encode_options(&[pppoe::address_option(me)]));
                self.pppoe_ppp(t, d, in_port, client, sid, pppoe::PPP_IPCP, req);
            }
        }
        let done = n.stage == Stage::Ipcp && n.ip_ours_acked && n.ip_theirs_acked;
        let st = self.st(d.id);
        if over || done {
            st.pppoe.neg.remove(&sid);
        } else {
            st.pppoe.neg.insert(sid, n);
        }
        Ok(())
    }

    // ---- the dialer ----

    fn pppoe_listen(&mut self, dev: DeviceId, on: bool) {
        let st = self.st(dev);
        st.pppoe.listening = on;
        st.pppoe.inbox.clear();
    }

    /// Waits for a packet the dialer wants, up to `timeout_us` of simulated time.
    fn pppoe_wait<F: Fn(&Heard) -> bool>(&mut self, t: &Topology, dev: DeviceId, timeout_us: u64, want: F) -> Option<Heard> {
        let take = |s: &mut Sim| -> Option<Heard> {
            let st = s.state.get_mut(&dev)?;
            let i = st.pppoe.inbox.iter().position(&want)?;
            Some(st.pppoe.inbox.remove(i))
        };
        if let Some(h) = take(&mut *self) {
            return Some(h);
        }
        let deadline = self.now.saturating_add(timeout_us);
        let has = |s: &Sim| s.state.get(&dev).is_some_and(|st| st.pppoe.inbox.iter().any(&want));
        self.advance(t, deadline, has);
        take(&mut *self)
    }

    /// Discovery: a PADI, and the PADO that answers it (asked twice before giving up).
    fn pppoe_find(&mut self, t: &Topology, d: &Device, port: usize, uniq: &[u8]) -> Option<Offer> {
        for _ in 0..2 {
            let padi = Discovery {
                code: pppoe::CODE_PADI,
                session_id: 0,
                tags: vec![Tag::new(pppoe::TAG_SERVICE_NAME, &[]), Tag::new(pppoe::TAG_HOST_UNIQ, uniq)],
            };
            self.pppoe_emit(t, d, port, MacAddr::BROADCAST, pppoe::ETHERTYPE_DISCOVERY, padi.encode());
            let want = |h: &Heard| matches!(&h.what, What::Discovery(x) if x.code == pppoe::CODE_PADO && x.tag(pppoe::TAG_HOST_UNIQ) == Some(uniq));
            if let Some(h) = self.pppoe_wait(t, d.id, WAIT_US, want)
                && let What::Discovery(x) = h.what
            {
                return Some(Offer {
                    ac: h.src,
                    name: x.tag_text(pppoe::TAG_AC_NAME).unwrap_or_default(),
                    cookie: x.tag(pppoe::TAG_AC_COOKIE).map(<[u8]>::to_vec),
                    service: x.tag(pppoe::TAG_SERVICE_NAME).map(<[u8]>::to_vec).unwrap_or_default(),
                });
            }
        }
        None
    }

    /// Whether some concentrator answers a PADI on this computer's cable, and which.
    pub fn pppoe_discover(&mut self, t: &Topology, dev: DeviceId) -> Option<(MacAddr, String)> {
        let d = t.device(dev)?;
        if !can_dial(d.kind) || !port_up(t, End { device: dev, port: 0 }) {
            return None;
        }
        self.seq += 1;
        let uniq = u32::try_from(self.seq & 0xffff_ffff).unwrap_or(0).to_be_bytes();
        self.pppoe_listen(dev, true);
        let found = self.pppoe_find(t, d, 0, &uniq);
        self.pppoe_listen(dev, false);
        found.map(|o| (o.ac, o.name))
    }

    /// Dials from a computer's first port: discovery, LCP, authentication and IPCP, as frames.
    /// The caller applies the result to the lab.
    pub fn pppoe_dial(&mut self, t: &Topology, dev: DeviceId, user: &str, password: &str) -> Result<DialResult, SimError> {
        self.pppoe_dial_port(t, dev, 0, &DialCreds::same(user, password))
    }

    /// Dials from one port of a computer or a router.
    pub fn pppoe_dial_port(&mut self, t: &Topology, dev: DeviceId, port: usize, creds: &DialCreds) -> Result<DialResult, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        if !can_dial(d.kind) {
            return Err(SimError::NoSource("only computers and routers dial PPPoE".into()));
        }
        if !port_up(t, End { device: dev, port }) {
            return Err(SimError::NoSource("no PPPoE server answered: the network port has no cable".into()));
        }
        self.pppoe_listen(dev, true);
        let mut opened = None;
        let r = self.pppoe_dial_run(t, d, port, creds, &mut opened);
        if r.is_err()
            && let Some((ac, session_id)) = opened
        {
            self.pppoe_padt(t, d, port, ac, session_id);
        }
        if r.is_err() {
            // What the failed try left on the wire (a Terminate-Request, a PADT) arrives now, not
            // in the middle of the next call.
            let until = self.now.saturating_add(50_000);
            self.advance(t, until, |_| false);
        }
        self.pppoe_listen(dev, false);
        r
    }

    fn pppoe_dial_run(
        &mut self,
        t: &Topology,
        d: &Device,
        port: usize,
        creds: &DialCreds,
        opened: &mut Option<(MacAddr, u16)>,
    ) -> Result<DialResult, SimError> {
        let err = |m: &str| SimError::NoSource(m.to_string());
        self.seq += 1;
        let uniq = u32::try_from(self.seq & 0xffff_ffff).unwrap_or(0).to_be_bytes();
        let offer = self
            .pppoe_find(t, d, port, &uniq)
            .ok_or_else(|| err("no PPPoE server answered: no access concentrator replied to the discovery request"))?;
        let ac = offer.ac;
        let mut tags = vec![Tag::new(pppoe::TAG_SERVICE_NAME, &offer.service), Tag::new(pppoe::TAG_HOST_UNIQ, &uniq)];
        if let Some(c) = &offer.cookie {
            tags.push(Tag::new(pppoe::TAG_AC_COOKIE, c));
        }
        let padr = Discovery { code: pppoe::CODE_PADR, session_id: 0, tags };
        let mut pads = None;
        for _ in 0..2 {
            self.pppoe_emit(t, d, port, ac, pppoe::ETHERTYPE_DISCOVERY, padr.encode());
            let want = |h: &Heard| {
                h.src == ac && matches!(&h.what, What::Discovery(x) if x.code == pppoe::CODE_PADS && x.tag(pppoe::TAG_HOST_UNIQ) == Some(&uniq[..]))
            };
            pads = self.pppoe_wait(t, d.id, WAIT_US, want);
            if pads.is_some() {
                break;
            }
        }
        let Some(Heard { what: What::Discovery(pads), .. }) = pads else {
            return Err(err("the access concentrator did not answer the session request"));
        };
        if pads.session_id == 0 {
            let why = pads.error_text().unwrap_or_else(|| "no reason given".to_string());
            return Err(SimError::NoSource(format!("the access concentrator refused the session: {why}")));
        }
        let sid = pads.session_id;
        *opened = Some((ac, sid));

        let magic = u32::from_be_bytes(uniq) | 1;
        let first = Control::new(pppoe::CONF_REQ, 1, pppoe::encode_options(&[pppoe::mru_option(pppoe::MRU), pppoe::magic_option(magic)]));
        self.pppoe_ppp(t, d, port, ac, sid, pppoe::PPP_LCP, first);
        let (mut ours, mut theirs) = (false, false);
        let mut chap: Option<bool> = None;
        let (mut authed, mut pap_sent) = (false, false);
        let (mut ip_sent, mut ip_id) = (false, 0u8);
        let mut my_addr = Ipv4Addr::UNSPECIFIED;
        let mut peer_addr: Option<Ipv4Addr> = None;
        let (mut ip_ours, mut ip_theirs) = (false, false);
        for _ in 0..48 {
            if ip_ours && ip_theirs {
                break;
            }
            let want = |h: &Heard| {
                h.src == ac
                    && match &h.what {
                        What::Ppp { session, .. } => *session == sid,
                        What::Discovery(x) => x.code == pppoe::CODE_PADT && x.session_id == sid,
                    }
            };
            let heard = self.pppoe_wait(t, d.id, WAIT_US, want).ok_or_else(|| err("the access concentrator stopped answering"))?;
            let (protocol, c) = match heard.what {
                What::Discovery(_) => return Err(err("the access concentrator ended the session (PADT)")),
                What::Ppp { protocol, packet, .. } => (protocol, packet),
            };
            match (protocol, c.code) {
                (pppoe::PPP_LCP, pppoe::CONF_REQ) => {
                    let opts = pppoe::decode_options(&c.data).unwrap_or_default();
                    chap = pppoe::opt(&opts, pppoe::LCP_AUTH).and_then(pppoe::parse_auth);
                    self.pppoe_ppp(t, d, port, ac, sid, pppoe::PPP_LCP, Control::new(pppoe::CONF_ACK, c.id, c.data.clone()));
                    theirs = true;
                }
                (pppoe::PPP_LCP, pppoe::CONF_ACK) if c.id == 1 => ours = true,
                (pppoe::PPP_LCP, pppoe::CONF_NAK | pppoe::CONF_REJ) => return Err(err("the access concentrator did not accept the link settings")),
                (pppoe::PPP_LCP, pppoe::TERM_REQ) => {
                    self.pppoe_ppp(t, d, port, ac, sid, pppoe::PPP_LCP, Control::new(pppoe::TERM_ACK, c.id, Vec::new()));
                    return Err(err("the access concentrator closed the link"));
                }
                (pppoe::PPP_LCP, pppoe::ECHO_REQ) => {
                    let m = magic.to_be_bytes().to_vec();
                    self.pppoe_ppp(t, d, port, ac, sid, pppoe::PPP_LCP, Control::new(pppoe::ECHO_REP, c.id, m));
                }
                (pppoe::PPP_CHAP, pppoe::CHAP_CHALLENGE) => {
                    if let Ok((value, _)) = pppoe::parse_chap_data(&c.data) {
                        let answer = pppoe::chap_response(c.id, &creds.chap_secret, &value);
                        let data = pppoe::chap_data(&answer, &creds.chap_name);
                        self.pppoe_ppp(t, d, port, ac, sid, pppoe::PPP_CHAP, Control::new(pppoe::CHAP_RESPONSE, c.id, data));
                    }
                }
                (pppoe::PPP_CHAP, pppoe::CHAP_SUCCESS) | (pppoe::PPP_PAP, pppoe::PAP_ACK) => authed = true,
                (pppoe::PPP_CHAP, pppoe::CHAP_FAILURE) => return Err(err("authentication failed: the server refused the response (CHAP)")),
                (pppoe::PPP_PAP, pppoe::PAP_NAK) => return Err(err("authentication failed: the server refused the name or password (PAP)")),
                (pppoe::PPP_IPCP, pppoe::CONF_REQ) => {
                    let opts = pppoe::decode_options(&c.data).unwrap_or_default();
                    peer_addr = pppoe::opt_address(&opts);
                    self.pppoe_ppp(t, d, port, ac, sid, pppoe::PPP_IPCP, Control::new(pppoe::CONF_ACK, c.id, c.data.clone()));
                    ip_theirs = true;
                }
                (pppoe::PPP_IPCP, pppoe::CONF_NAK) => {
                    let opts = pppoe::decode_options(&c.data).unwrap_or_default();
                    if let Some(a) = pppoe::opt_address(&opts) {
                        my_addr = a;
                        ip_id = ip_id.wrapping_add(1);
                        let req = Control::new(pppoe::CONF_REQ, ip_id, pppoe::encode_options(&[pppoe::address_option(a)]));
                        self.pppoe_ppp(t, d, port, ac, sid, pppoe::PPP_IPCP, req);
                    }
                }
                (pppoe::PPP_IPCP, pppoe::CONF_ACK) if c.id == ip_id && !my_addr.is_unspecified() => ip_ours = true,
                (pppoe::PPP_IPCP, pppoe::CONF_REJ) => return Err(err("the access concentrator did not accept the address request")),
                _ => {}
            }
            if ours && theirs {
                match chap {
                    Some(false) if !pap_sent => {
                        pap_sent = true;
                        self.pppoe_ppp(
                            t,
                            d,
                            port,
                            ac,
                            sid,
                            pppoe::PPP_PAP,
                            Control::new(pppoe::PAP_REQUEST, 1, pppoe::pap_request(&creds.pap_name, &creds.pap_secret)),
                        );
                    }
                    None => authed = true,
                    _ => {}
                }
            }
            if authed && !ip_sent {
                ip_sent = true;
                ip_id = 1;
                let req = Control::new(pppoe::CONF_REQ, ip_id, pppoe::encode_options(&[pppoe::address_option(Ipv4Addr::UNSPECIFIED)]));
                self.pppoe_ppp(t, d, port, ac, sid, pppoe::PPP_IPCP, req);
            }
        }
        let gateway = peer_addr.filter(|_| ip_ours && ip_theirs).ok_or_else(|| err("the PPP negotiation did not finish"))?;
        Ok(DialResult {
            session: sid,
            server_mac: ac,
            ac_name: offer.name,
            address: my_addr,
            gateway,
            auth: if chap == Some(true) { PppAuth::Chap } else { PppAuth::Pap },
        })
    }

    /// A keep-alive over the session in the lab: an LCP Echo-Request and its reply. False when
    /// there is no session, no cable, or no answer (the concentrator is gone or no longer knows
    /// the session).
    pub fn pppoe_probe(&mut self, t: &Topology, dev: DeviceId) -> bool {
        self.pppoe_probe_port(t, dev, 0)
    }

    /// [`Sim::pppoe_probe`] over the session of one port (a router's dialer runs over any).
    pub fn pppoe_probe_port(&mut self, t: &Topology, dev: DeviceId, port: usize) -> bool {
        let Some(d) = t.device(dev) else { return false };
        let Some(s) = client_session(d, port).cloned() else { return false };
        if s.id == 0 || !can_dial(d.kind) || !port_up(t, End { device: dev, port }) {
            return false;
        }
        self.pppoe_listen(dev, true);
        let id = u8::try_from(self.ip_id() & 0xff).unwrap_or(1);
        let magic = magic_of(d, s.server_mac).to_be_bytes().to_vec();
        self.pppoe_ppp(t, d, port, s.server_mac, s.id, pppoe::PPP_LCP, Control::new(pppoe::ECHO_REQ, id, magic));
        let (ac, sid) = (s.server_mac, s.id);
        let want = |h: &Heard| {
            h.src == ac
                && match &h.what {
                    What::Ppp { session, protocol, packet } => *session == sid && *protocol == pppoe::PPP_LCP && packet.code == pppoe::ECHO_REP,
                    What::Discovery(x) => x.code == pppoe::CODE_PADT && x.session_id == sid,
                }
        };
        let heard = self.pppoe_wait(t, dev, PROBE_US, want);
        self.pppoe_listen(dev, false);
        matches!(heard, Some(Heard { what: What::Ppp { .. }, .. }))
    }

    /// The dialer ends its session: LCP Terminate-Request and a PADT (queued: the caller runs
    /// the clock a little to let them arrive).
    pub fn pppoe_hangup(&mut self, t: &Topology, dev: DeviceId) {
        self.pppoe_hangup_port(t, dev, 0);
    }

    /// [`Sim::pppoe_hangup`] for the session of one port.
    pub fn pppoe_hangup_port(&mut self, t: &Topology, dev: DeviceId, port: usize) {
        let Some(d) = t.device(dev) else { return };
        let Some(s) = client_session(d, port).cloned() else { return };
        if s.id == 0 || !can_dial(d.kind) || !port_up(t, End { device: dev, port }) {
            return;
        }
        self.pppoe_ppp(t, d, port, s.server_mac, s.id, pppoe::PPP_LCP, Control::new(pppoe::TERM_REQ, 1, Vec::new()));
        self.pppoe_padt(t, d, port, s.server_mac, s.id);
    }

    /// A concentrator whose service was switched off ends every session it holds with a PADT.
    pub fn pppoe_server_down(&mut self, t: &Topology, server: DeviceId) {
        let _ = self.pppoe_end_sessions(t, server, false);
    }

    /// A router whose PPPoE settings changed ends the sessions it no longer answers (a port that
    /// stopped enabling PPPoE, a group or a template that is gone) with a PADT.
    pub fn pppoe_unserved_down(&mut self, t: &Topology, server: DeviceId) -> usize {
        self.pppoe_end_sessions(t, server, true)
    }

    fn pppoe_end_sessions(&mut self, t: &Topology, server: DeviceId, only_unserved: bool) -> usize {
        let Some(d) = t.device(server) else { return 0 };
        if !only_unserved {
            self.st(server).pppoe.neg.clear();
        }
        let ends: Vec<(MacAddr, u16, usize)> = calls(t)
            .filter(|(_, _, s)| s.server == server && s.id != 0)
            .filter_map(|(c, cport, s)| {
                let client = c.ports.get(cport)?.mac;
                let sport = d.ports.iter().position(|p| p.mac == s.server_mac)?;
                (!only_unserved || concentrator(d, sport).is_none()).then_some((client, s.id, sport))
            })
            .collect();
        let n = ends.len();
        for (client, id, sport) in ends {
            self.pppoe_padt(t, d, sport, client, id);
        }
        n
    }

    /// Sessions the dialers heard end (a PADT or a Terminate-Request): the engine drops them
    /// from the lab. Each is the caller and the session id.
    pub fn take_pppoe_ended(&mut self) -> Vec<(DeviceId, u16)> {
        std::mem::take(&mut self.pppoe_ended)
    }
}
