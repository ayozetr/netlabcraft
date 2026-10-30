//! Site-to-site VPNs: GRE tunnels (RFC 2784) and basic IPsec ESP (RFC 4303).
//!
//! A tunnel is a layer-3 interface without a port. Its line protocol is up when it is not shut
//! down, its source names an up interface (or an address of one) and its destination is
//! reachable over a physical interface: through a connected or static route, or through a route
//! OSPF, RIP or EIGRP learned over the physical network (worked out with the tunnels that
//! depend on learned routes down). A tunnel whose destination, once it is up, would be reached
//! through a tunnel instead (the routing protocol learned it through the tunnel itself) goes
//! down again for recursive routing. Packets routed
//! out of it get an outer IPv4 header (source → destination, protocol 47) and a GRE header,
//! and are routed again; a packet whose outer destination would itself go into a tunnel is
//! dropped (recursive routing), so nothing is encapsulated twice. The far router accepts GRE
//! for an up tunnel whose source and destination mirror the packet's addresses and routes the
//! inner packet as if it had arrived on that tunnel. Routing protocols see the two tunnel ends
//! as a point-to-point link.
//!
//! IPsec: a crypto map applied to an interface protects the packets leaving it that its access
//! list permits. The first such packet towards a peer without a security association starts
//! IKEv1 (RFC 2409) on the wire, over UDP port 500, and is dropped: main mode (six messages:
//! policy proposal and choice, key exchange and nonces, then identities and hashes encrypted
//! with the pre-shared key) and quick mode (three encrypted messages agreeing the ESP
//! transform). The association comes up only when quick mode completes. What each side
//! accepts is decided from both routers' configuration, at the message where a real exchange
//! would find out: ISAKMP policies that don't agree are refused at main mode message 2,
//! pre-shared keys that differ fail authentication at message 5, transform sets or crypto map
//! entries that don't match are refused at quick mode message 2; the refusing side sends a
//! notification (in the clear here, so the event list shows it). Protected packets travel as
//! ESP whose cipher is a keyed XOR, **not real encryption** (see `packetcraft_wire::vpn`).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use packetcraft_core::DeviceId;

use packetcraft_core::MacAddr;
use packetcraft_model::vpn::{CryptoMapEntry, Tunnel, TunnelSource};
use packetcraft_model::{Device, DeviceKind, End, NextHop, Topology, ifname};
use packetcraft_wire::ike::{self, Isakmp, Payload, Proposal, Transform};
use packetcraft_wire::tcp::Tcp;
use packetcraft_wire::vpn::{Esp, Gre, NEXT_IPV4, PROTO_ESP, PROTO_GRE};
use packetcraft_wire::{ETHERTYPE_IPV4, Ipv4, PROTO_TCP, PROTO_UDP, Udp};

use crate::{L3If, SVI_PORT, Sim};

/// Tunnel `n` is reported as port `TUNNEL_BASE + n`: far beyond any real port index, and below
/// [`SVI_PORT`] even on 32-bit targets (tunnel numbers stop at 2^31 − 1).
pub const TUNNEL_BASE: usize = usize::MAX / 4;

/// The pseudo port index of tunnel `n`.
pub fn tunnel_port(n: u32) -> usize {
    TUNNEL_BASE.saturating_add(usize::try_from(n).unwrap_or(0))
}

/// The tunnel number behind a pseudo port index, if it is one.
pub fn tunnel_of(port: usize) -> Option<u32> {
    if port == SVI_PORT {
        return None;
    }
    port.checked_sub(TUNNEL_BASE).and_then(|n| u32::try_from(n).ok())
}

pub fn is_tunnel(port: usize) -> bool {
    tunnel_of(port).is_some()
}

/// The MAC address tunnel interfaces report (they never send frames themselves).
fn tunnel_mac(d: &Device) -> MacAddr {
    MacAddr::local(d.id, 0xfffd)
}

/// The tunnels a router has configured with an address and not shut down.
pub(crate) fn tunnel_ifaces(d: &Device) -> Vec<L3If> {
    if d.kind != DeviceKind::Router {
        return Vec::new();
    }
    d.tunnels
        .iter()
        .filter(|t| !t.shutdown)
        .filter_map(|t| t.ip.map(|net| L3If { port: tunnel_port(t.number), sub: None, vlan: None, net, mac: tunnel_mac(d), name: t.name() }))
        .collect()
}

/// Whether `dst` is reachable through a connected or static route over the physical up
/// interfaces `phys`.
fn phys_routable(d: &Device, dst: Ipv4Addr, phys: &[L3If]) -> bool {
    phys.iter().any(|i| i.net.contains(dst))
        || d.routes.iter().filter(|r| r.net.contains(dst)).any(|r| match &r.via {
            NextHop::Address(a) => phys.iter().any(|i| i.net.contains(*a)),
            NextHop::Interface(n) => phys.iter().any(|i| same_name(&i.name, n)),
        })
}

/// Whether two interface names (any spelling) are the same interface.
pub(crate) fn same_name(a: &str, b: &str) -> bool {
    if let (Some(x), Some(y)) = (packetcraft_model::vpn::tunnel_number(a), packetcraft_model::vpn::tunnel_number(b)) {
        return x == y;
    }
    match (ifname::parse(a), ifname::parse(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// Why a tunnel is down when nothing routes to its destination.
const NO_ROUTE: &str = "no route to the tunnel destination";
/// Why a tunnel is down when its destination is reached through a tunnel.
const RECURSIVE: &str = "recursive routing: the destination is learned through a tunnel";

type TunnelKey = (DeviceId, u32);

thread_local! {
    /// While the verdicts below are worked out: the tunnels that count as up through learned
    /// routes (the others that depend on learned routes count as down).
    static PHASE: RefCell<Option<BTreeSet<TunnelKey>>> = const { RefCell::new(None) };
    /// For the lab with this fingerprint: each tunnel that depends on learned routes, up or why
    /// not.
    static VERDICTS: RefCell<Option<(u64, BTreeMap<TunnelKey, Result<(), &'static str>>)>> = const { RefCell::new(None) };
}

/// Whether a tunnel whose destination only learned routes reach is up.
fn learned_verdict(t: &Topology, d: &Device, tun: &Tunnel) -> Result<(), &'static str> {
    let key = (d.id, tun.number);
    if let Some(up) = PHASE.try_with(|p| p.try_borrow().ok().and_then(|p| p.as_ref().map(|s| s.contains(&key)))).ok().flatten() {
        return if up { Ok(()) } else { Err(NO_ROUTE) };
    }
    let f = crate::ospf::fingerprint(t);
    let cached = VERDICTS
        .try_with(|v| v.try_borrow().ok().and_then(|v| v.as_ref().filter(|(at, _)| *at == f).map(|(_, m)| m.get(&key).copied())))
        .ok()
        .flatten();
    if let Some(v) = cached {
        return v.unwrap_or(Err(NO_ROUTE));
    }
    let verdicts = work_out_verdicts(t);
    let mine = verdicts.get(&key).copied().unwrap_or(Err(NO_ROUTE));
    let _ = VERDICTS.try_with(|v| {
        if let Ok(mut v) = v.try_borrow_mut() {
            *v = Some((f, verdicts));
        }
    });
    mine
}

/// Sets the tunnels that count as up through learned routes (`None`: back to the verdicts).
fn set_phase(up: Option<BTreeSet<TunnelKey>>) {
    let _ = PHASE.try_with(|p| {
        if let Ok(mut p) = p.try_borrow_mut() {
            *p = up;
        }
    });
}

/// Where each router's routing protocols send a destination: the outgoing interface.
fn learned_out(t: &Topology, d: &Device, dst: Ipv4Addr) -> Option<L3If> {
    let e = if d.eigrp.is_some() { crate::eigrp::compute(t).routes.remove(&d.id).unwrap_or_default() } else { Vec::new() };
    let o = if d.ospf.is_some() { crate::ospf::compute(t).routes.remove(&d.id).unwrap_or_default() } else { Vec::new() };
    let r = if d.rip.is_some() { crate::rip::compute(t).routes.remove(&d.id).unwrap_or_default() } else { Vec::new() };
    crate::route_with(t, d, dst, crate::flow_hash(Ipv4Addr::UNSPECIFIED, dst), &e, &o, &r).map(|(i, _)| i)
}

/// Every tunnel that depends on learned routes: first with all of them down, the ones whose
/// destination the protocols then reach over a physical interface come up; with those up, any
/// whose destination is now reached through a tunnel goes down for recursive routing.
fn work_out_verdicts(t: &Topology) -> BTreeMap<TunnelKey, Result<(), &'static str>> {
    let mut candidates: Vec<(DeviceId, u32, Ipv4Addr)> = Vec::new();
    for d in t.devices.iter().filter(|d| d.kind == DeviceKind::Router && !d.tunnels.is_empty()) {
        let phys = crate::phys_up_ifaces(t, d);
        for tun in &d.tunnels {
            if let (Ok(_), Some(dst)) = (static_state(d, tun, &phys), tun.destination)
                && !phys_routable(d, dst, &phys)
            {
                candidates.push((d.id, tun.number, dst));
            }
        }
    }
    let mut out = BTreeMap::new();
    if candidates.is_empty() {
        return out;
    }
    set_phase(Some(BTreeSet::new()));
    let mut up = BTreeSet::new();
    for (id, n, dst) in &candidates {
        let Some(d) = t.device(*id) else { continue };
        if learned_out(t, d, *dst).is_some_and(|i| !is_tunnel(i.port)) {
            up.insert((*id, *n));
        }
    }
    set_phase(Some(up.clone()));
    for (id, n, dst) in &candidates {
        let verdict = if !up.contains(&(*id, *n)) {
            Err(NO_ROUTE)
        } else {
            match t.device(*id).and_then(|d| learned_out(t, d, *dst)) {
                Some(i) if is_tunnel(i.port) => Err(RECURSIVE),
                Some(_) => Ok(()),
                None => Err(NO_ROUTE),
            }
        };
        out.insert((*id, *n), verdict);
    }
    set_phase(None);
    out
}

/// A tunnel's state from its own settings: shut down, source, destination configured.
fn static_state(_d: &Device, tun: &Tunnel, phys: &[L3If]) -> Result<(Ipv4Addr, Ipv4Addr), &'static str> {
    if tun.shutdown {
        return Err("administratively down");
    }
    let src = match &tun.source {
        None => return Err("no tunnel source"),
        Some(TunnelSource::Interface(n)) => {
            phys.iter().find(|i| same_name(&i.name, n)).map(|i| i.net.addr).ok_or("the source interface is down or has no address")?
        }
        Some(TunnelSource::Address(a)) => {
            phys.iter().find(|i| i.net.addr == *a).map(|i| i.net.addr).ok_or("the source address is not on an up interface")?
        }
    };
    let dst = tun.destination.ok_or("no tunnel destination")?;
    if tun.ip.is_some_and(|n| n.contains(dst)) {
        return Err("the destination is inside the tunnel's own network");
    }
    Ok((src, dst))
}

/// A tunnel's endpoints (source, destination) when its line protocol is up, else why not.
/// `phys` are the router's physical up interfaces.
pub fn tunnel_state(t: &Topology, d: &Device, tun: &Tunnel, phys: &[L3If]) -> Result<(Ipv4Addr, Ipv4Addr), &'static str> {
    let (src, dst) = static_state(d, tun, phys)?;
    if !phys_routable(d, dst, phys) {
        learned_verdict(t, d, tun)?;
    }
    Ok((src, dst))
}

/// The tunnels of a router that are up, given its physical up interfaces.
pub(crate) fn up_tunnels(t: &Topology, d: &Device, phys: &[L3If]) -> Vec<L3If> {
    tunnel_ifaces(d)
        .into_iter()
        .filter(|i| tunnel_of(i.port).and_then(|n| d.tunnels.iter().find(|x| x.number == n)).is_some_and(|x| tunnel_state(t, d, x, phys).is_ok()))
        .collect()
}

/// `show interfaces tunnel N`: the tunnel's endpoints when up, else why its line protocol is
/// down.
pub fn tunnel_status(t: &Topology, d: &Device, number: u32) -> Result<(Ipv4Addr, Ipv4Addr), String> {
    let tun = d.tunnels.iter().find(|x| x.number == number).ok_or_else(|| format!("no interface Tunnel{number}"))?;
    let phys = crate::phys_up_ifaces(t, d);
    tunnel_state(t, d, tun, &phys).map_err(str::to_string)
}

/// The far ends of an up tunnel interface: every router tunnel, up, whose source and
/// destination mirror this one's. Reported as (device, tunnel pseudo port).
pub(crate) fn tunnel_peers(t: &Topology, d: &Device, ifc: &L3If) -> BTreeSet<End> {
    let mut out = BTreeSet::new();
    let Some(tun) = tunnel_of(ifc.port).and_then(|n| d.tunnels.iter().find(|x| x.number == n)) else { return out };
    let Ok((src, dst)) = tunnel_state(t, d, tun, &crate::phys_up_ifaces(t, d)) else { return out };
    for b in t.devices.iter().filter(|b| b.id != d.id && b.kind == DeviceKind::Router && !b.tunnels.is_empty()) {
        let phys = crate::phys_up_ifaces(t, b);
        for bt in b.tunnels.iter().filter(|x| x.ip.is_some()) {
            if tunnel_state(t, b, bt, &phys) == Ok((dst, src)) {
                out.insert(End { device: b.id, port: tunnel_port(bt.number) });
            }
        }
    }
    out
}

/// One IPsec peer's security association, as `show crypto` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpsecSa {
    pub local: Ipv4Addr,
    pub peer: Ipv4Addr,
    /// The interface and crypto map that protect the traffic.
    pub iface: String,
    pub map: String,
    pub seq: u32,
    pub spi_out: u32,
    pub spi_in: u32,
    pub encaps: u64,
    pub decaps: u64,
    /// Packets dropped (no association, failed integrity check).
    pub drops: u64,
    /// Why the association could not be set up the last time IKE tried.
    pub failed: Option<String>,
    /// Where IKE stands with this peer.
    pub phase: IkePhase,
    /// Whether this router started the exchange.
    pub initiator: bool,
    pub icookie: [u8; 8],
    pub rcookie: [u8; 8],
    /// When the exchange started (simulated µs).
    pub started_us: u64,
    /// Quick mode's message ID.
    pub qm_id: u32,
}

/// Where an IKE exchange with a peer stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IkePhase {
    /// Nothing tried yet.
    #[default]
    Idle,
    /// Main mode: the last message sent or taken (1 to 6).
    Main(u8),
    /// Quick mode: the last message sent or taken (1 to 3).
    Quick(u8),
    Established,
    Failed,
}

impl IkePhase {
    pub fn describe(self) -> String {
        match self {
            IkePhase::Idle => "idle".into(),
            IkePhase::Main(n) => format!("main mode, message {n}"),
            IkePhase::Quick(n) => format!("quick mode, message {n}"),
            IkePhase::Established => "established".into(),
            IkePhase::Failed => "failed".into(),
        }
    }
}

impl Default for IpsecSa {
    fn default() -> Self {
        IpsecSa {
            local: Ipv4Addr::UNSPECIFIED,
            peer: Ipv4Addr::UNSPECIFIED,
            iface: String::new(),
            map: String::new(),
            seq: 0,
            spi_out: 0,
            spi_in: 0,
            encaps: 0,
            decaps: 0,
            drops: 0,
            failed: None,
            phase: IkePhase::Idle,
            initiator: false,
            icookie: [0; 8],
            rcookie: [0; 8],
            started_us: 0,
            qm_id: 0,
        }
    }
}

/// An exchange that hasn't finished (or failed) this long is started again by the next packet.
const IKE_RETRY_US: u64 = 2_000_000;

/// Most IPsec peers tracked per router.
const MAX_SAS: usize = 256;

/// A security association both routers' configuration agrees on.
struct Agreed {
    key: String,
    spi_out: u32,
    spi_in: u32,
}

/// The SPI one direction uses: derived from the key and both addresses, never below 256.
fn spi(key: &str, from: Ipv4Addr, to: Ipv4Addr) -> u32 {
    let mut seed = key.as_bytes().to_vec();
    seed.extend_from_slice(&from.octets());
    seed.extend_from_slice(&to.octets());
    let h = packetcraft_wire::md5::md5(&seed);
    u32::from_be_bytes([h[0], h[1], h[2], h[3]]) | 0x100
}

/// The device that owns an address.
fn owner<'a>(t: &'a Topology, me: &Device, addr: Ipv4Addr) -> Option<&'a Device> {
    t.devices.iter().find(|x| x.id != me.id && crate::l3_ifaces(x).iter().any(|i| i.net.addr == addr))
}

/// What this router itself needs before it can start IKE for a crypto map entry: a peer, a
/// transform set that exists and a key for the peer.
fn local_check(d: &Device, e: &CryptoMapEntry) -> Result<(Ipv4Addr, String), String> {
    let peer = e.peer.ok_or_else(|| format!("crypto map {} {} has no peer (`set peer`)", e.map, e.seq))?;
    let ts_name = e.transform_set.as_deref().ok_or_else(|| format!("crypto map {} {} has no transform set", e.map, e.seq))?;
    d.crypto.transform_set(ts_name).ok_or_else(|| format!("transform set {ts_name} does not exist"))?;
    let key = d.crypto.key_for(peer).ok_or_else(|| format!("no ISAKMP key for peer {peer}"))?;
    Ok((peer, key.to_string()))
}

/// Main mode messages 1 and 2: an ISAKMP policy both routers have (when both have some).
fn policy_check(a: &Device, b: &Device) -> Result<(), String> {
    if !a.crypto.policies.is_empty() && !b.crypto.policies.is_empty() {
        let agreed = a.crypto.policies.iter().find(|x| b.crypto.policies.iter().any(|y| x.agrees(y))).ok_or("no ISAKMP policy matches the peer's")?;
        if agreed.authentication != "pre-share" {
            return Err("the matching ISAKMP policy does not use pre-shared keys".into());
        }
    }
    Ok(())
}

/// Main mode message 5: both routers hold the same pre-shared key for each other.
fn auth_check(a: &Device, a_addr: Ipv4Addr, b: &Device, b_addr: Ipv4Addr) -> Result<(), String> {
    let key = a.crypto.key_for(b_addr).ok_or_else(|| format!("no ISAKMP key for peer {b_addr}"))?;
    let theirs = b.crypto.key_for(a_addr).ok_or_else(|| format!("the peer has no ISAKMP key for {a_addr}"))?;
    if theirs != key {
        return Err("the pre-shared keys differ (ISAKMP authentication failed)".into());
    }
    Ok(())
}

/// Quick mode messages 1 and 2: the responder has a crypto map entry back to the initiator,
/// on the interface the exchange reached, with the same transform set.
fn quick_check(a: &Device, a_addr: Ipv4Addr, e: &CryptoMapEntry, b: &Device, b_addr: Ipv4Addr) -> Result<(), String> {
    let ts = e.transform_set.as_deref().and_then(|n| a.crypto.transform_set(n)).ok_or("the crypto map entry has no usable transform set")?;
    let p_if = crate::l3_ifaces(b).into_iter().find(|i| i.net.addr == b_addr).map(|i| i.name).unwrap_or_default();
    let p_map =
        b.crypto.applied.iter().find(|(n, _)| same_name(n, &p_if)).map(|(_, m)| m).ok_or_else(|| format!("the peer has no crypto map on {p_if}"))?;
    let pe =
        b.crypto.map(p_map).find(|x| x.peer == Some(a_addr)).ok_or_else(|| format!("the peer's crypto map {p_map} has no entry for {a_addr}"))?;
    let p_ts =
        pe.transform_set.as_deref().and_then(|n| b.crypto.transform_set(n)).ok_or("the peer's crypto map entry has no usable transform set")?;
    if p_ts.transforms != ts.transforms || p_ts.transport != ts.transport {
        return Err(format!("transform sets differ ({} here, {} at the peer)", ts.transforms.join(" "), p_ts.transforms.join(" ")));
    }
    Ok(())
}

/// Decides the association a crypto map entry asks for, from both routers' configuration: the
/// checks of every IKE stage in the order the exchange makes them.
fn negotiate(t: &Topology, d: &Device, local: Ipv4Addr, e: &CryptoMapEntry) -> Result<Agreed, String> {
    let (peer, key) = local_check(d, e)?;
    let p = owner(t, d, peer).ok_or_else(|| format!("no device has the address {peer}"))?;
    policy_check(d, p)?;
    auth_check(d, local, p, peer)?;
    quick_check(d, local, e, p, peer)?;
    Ok(Agreed { spi_out: spi(&key, local, peer), spi_in: spi(&key, peer, local), key })
}

/// ISAKMP attribute values for a policy (RFC 2409 appendix A).
fn policy_transform(number: u8, p: Option<&packetcraft_model::vpn::IsakmpPolicy>) -> Transform {
    let words: Vec<&str> = p.map_or(vec!["des"], |p| p.encryption.split_whitespace().collect());
    let cipher = match words.first().copied().unwrap_or("des") {
        "3des" => 5,
        w if w.starts_with("aes") => 7,
        _ => 1,
    };
    let mut attrs = vec![(1, cipher)];
    if cipher == 7 {
        attrs.push((14, words.get(1).and_then(|k| k.parse().ok()).unwrap_or(128)));
    }
    let hash = match p.map_or("sha", |p| p.hash.as_str()) {
        "md5" => 1,
        "sha256" => 4,
        "sha384" => 5,
        "sha512" => 6,
        _ => 2,
    };
    let auth = if p.is_none_or(|p| p.authentication == "pre-share") { 1 } else { 3 };
    let life = p.and_then(|p| p.lifetime).unwrap_or(86_400);
    attrs.extend([(2, hash), (3, auth), (4, u16::from(p.map_or(1, |p| p.group))), (11, 1), (12, u16::try_from(life).unwrap_or(u16::MAX))]);
    Transform { number, id: 1, attrs }
}

/// ESP transform from a transform set (RFC 2407 §4.4.4 and §4.5).
fn esp_transform(ts: Option<&packetcraft_model::vpn::TransformSet>) -> Transform {
    let has = |w: &str| ts.is_some_and(|t| t.transforms.iter().any(|x| x == w));
    let id = if has("esp-aes") {
        12
    } else if has("esp-3des") {
        3
    } else if has("esp-null") {
        11
    } else {
        2
    };
    let mut attrs = vec![(1, 1), (2, 3_600), (4, if ts.is_some_and(|t| t.transport) { 2 } else { 1 })];
    if has("esp-sha-hmac") {
        attrs.push((5, 2));
    } else if has("esp-md5-hmac") {
        attrs.push((5, 1));
    }
    if id == 12 {
        attrs.push((6, 128));
    }
    Transform { number: 1, id, attrs }
}

fn id_payload(a: Ipv4Addr) -> Payload {
    Payload::Id { kind: 1, protocol: 17, port: ike::ISAKMP_PORT, data: a.octets().to_vec() }
}

/// Whether a packet is ISAKMP (UDP port 500), which crypto maps never protect.
fn is_isakmp(ip: &Ipv4) -> bool {
    ip.protocol == PROTO_UDP
        && Udp::decode(&ip.payload, ip.src, ip.dst).is_ok_and(|u| u.dst_port == ike::ISAKMP_PORT && u.src_port == ike::ISAKMP_PORT)
}

/// Whether an access list permits a packet (an explicit permit; a missing list permits
/// nothing).
pub(crate) fn acl_permits(d: &Device, name: &str, ip: &Ipv4) -> bool {
    let Some(acl) = d.acls.iter().find(|a| a.name == name) else { return false };
    let (src_port, dst_port) = match ip.protocol {
        PROTO_UDP => Udp::decode(&ip.payload, ip.src, ip.dst).map_or((None, None), |u| (Some(u.src_port), Some(u.dst_port))),
        PROTO_TCP => Tcp::decode(&ip.payload, ip.src, ip.dst).map_or((None, None), |u| (Some(u.src_port), Some(u.dst_port))),
        _ => (None, None),
    };
    let key = packetcraft_model::acl::PacketKey { protocol: ip.protocol, src: ip.src, dst: ip.dst, src_port, dst_port };
    acl.evaluate(&key).0
}

impl Sim {
    /// Sends a datagram into a tunnel: GRE-encapsulated, then routed to the far end.
    pub(crate) fn gre_out(&mut self, t: &Topology, d: &Device, ifc: &L3If, datagram: Vec<u8>) -> Result<(), String> {
        let tun = tunnel_of(ifc.port).and_then(|n| d.tunnels.iter().find(|x| x.number == n)).ok_or("the tunnel is gone")?;
        let (src, dst) = tunnel_state(t, d, tun, &crate::phys_up_ifaces(t, d)).map_err(|e| format!("{} is down: {e}", tun.name()))?;
        let (out, nh) = self.route(t, d, dst).ok_or_else(|| format!("no route to tunnel destination {dst}"))?;
        if is_tunnel(out.port) {
            return Err(format!("{}: destination {dst} is routed through a tunnel (recursive routing); dropped", tun.name()));
        }
        let gre = Gre { protocol: ETHERTYPE_IPV4, checksum: false, payload: datagram };
        let mut outer = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_GRE, src, dst, payload: gre.encode() }.encode();
        crate::qos::copy_ds(&gre.payload, &mut outer);
        if self.state.get(&d.id).is_some_and(|s| s.debug_packet) {
            self.log(d.id, &format!("IP debug: {} encapsulated in GRE {src} → {dst}, out {}", tun.name(), out.name));
        }
        self.transmit(t, d, &out, nh, outer)
    }

    /// A GRE packet addressed to this router: the inner packet arrives on the matching tunnel.
    pub(crate) fn gre_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4) -> Result<(), String> {
        let g = Gre::decode(&ip.payload).map_err(|e| e.to_string())?;
        if g.protocol != ETHERTYPE_IPV4 {
            return Err(format!("GRE carrying protocol type 0x{:04x} is not understood", g.protocol));
        }
        let phys = crate::phys_up_ifaces(t, d);
        let ifc = tunnel_ifaces(d)
            .into_iter()
            .find(|i| {
                tunnel_of(i.port)
                    .and_then(|n| d.tunnels.iter().find(|x| x.number == n))
                    .is_some_and(|x| tunnel_state(t, d, x, &phys) == Ok((ip.dst, ip.src)))
            })
            .ok_or_else(|| format!("GRE from {}: no tunnel here is up with that destination and source {}", ip.src, ip.dst))?;
        self.ip_in(t, d, &ifc, &g.payload)
    }

    /// IPsec on the way out of `out`: the ESP packet and the peer to route it to when a crypto
    /// map protects the datagram, `None` when it leaves in the clear. Without a security
    /// association the packet starts IKE and is dropped.
    pub(crate) fn crypto_out(&mut self, t: &Topology, d: &Device, out: &L3If, datagram: &[u8]) -> Result<Option<(Vec<u8>, Ipv4Addr)>, String> {
        let Some(map) = d.crypto.applied.iter().find(|(n, _)| same_name(n, &out.name)).map(|(_, m)| m.clone()) else { return Ok(None) };
        let Ok(ip) = Ipv4::decode(datagram) else { return Ok(None) };
        if ip.protocol == PROTO_ESP || is_isakmp(&ip) {
            return Ok(None);
        }
        let Some(entry) = d.crypto.map(&map).find(|e| e.acl.as_deref().is_some_and(|a| acl_permits(d, a, &ip))).cloned() else { return Ok(None) };
        let peer = entry.peer.unwrap_or(Ipv4Addr::UNSPECIFIED);
        let local = out.net.addr;
        let now = self.now;
        let st = self.st(d.id);
        if !st.ipsec.contains_key(&peer) && st.ipsec.len() >= MAX_SAS {
            st.ipsec.clear();
        }
        let sa = st.ipsec.entry(peer).or_default();
        sa.local = local;
        sa.peer = peer;
        sa.iface = out.name.clone();
        sa.map = map;
        sa.seq = entry.seq;
        if let Err(why) = local_check(d, &entry) {
            sa.failed = Some(why.clone());
            sa.phase = IkePhase::Failed;
            sa.drops = sa.drops.saturating_add(1);
            return Err(format!("IPsec: no security association with {peer}: {why}"));
        }
        if sa.phase != IkePhase::Established {
            sa.drops = sa.drops.saturating_add(1);
            let busy =
                matches!(sa.phase, IkePhase::Main(_) | IkePhase::Quick(_) | IkePhase::Failed) && now.saturating_sub(sa.started_us) < IKE_RETRY_US;
            if busy {
                let why = match (&sa.failed, sa.phase) {
                    (Some(w), IkePhase::Failed) => w.clone(),
                    _ => format!("IKE negotiation in progress ({})", sa.phase.describe()),
                };
                return Err(format!("IPsec: no security association with {peer}: {why}"));
            }
            self.ike_start(t, d, local, peer);
            return Err(format!("IPsec: no security association with {peer} yet: IKE main mode started"));
        }
        // Established: the configuration may have changed since.
        match negotiate(t, d, local, &entry) {
            Err(why) => {
                let sa = self.st(d.id).ipsec.entry(peer).or_default();
                sa.failed = Some(why.clone());
                sa.phase = IkePhase::Failed;
                sa.drops = sa.drops.saturating_add(1);
                Err(format!("IPsec: no security association with {peer}: {why}"))
            }
            Ok(a) => {
                let sa = self.st(d.id).ipsec.entry(peer).or_default();
                sa.failed = None;
                sa.spi_out = a.spi_out;
                sa.spi_in = a.spi_in;
                sa.encaps = sa.encaps.saturating_add(1);
                let seq = u32::try_from(sa.encaps).unwrap_or(u32::MAX);
                let esp = Esp::seal(a.key.as_bytes(), a.spi_out, seq, NEXT_IPV4, datagram);
                let id = self.ip_id();
                let mut outer = Ipv4 { id, ttl: 255, protocol: PROTO_ESP, src: local, dst: peer, payload: esp.encode() }.encode();
                crate::qos::copy_ds(datagram, &mut outer);
                Ok(Some((outer, peer)))
            }
        }
    }

    /// Sends an ISAKMP message to a peer (UDP 500 both ways), waiting for ARP if it must.
    fn ike_send(&mut self, t: &Topology, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, m: &Isakmp) {
        let udp = Udp { src_port: ike::ISAKMP_PORT, dst_port: ike::ISAKMP_PORT, payload: m.encode() };
        let ip = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_UDP, src: local, dst: peer, payload: udp.encode(local, peer) };
        let held = self.hold_for_arp;
        self.hold_for_arp = true;
        let _ = self.send_ip(t, d, ip);
        self.hold_for_arp = held;
    }

    /// Main mode message 1: this router's ISAKMP policies as proposals.
    fn ike_start(&mut self, t: &Topology, d: &Device, local: Ipv4Addr, peer: Ipv4Addr) {
        let now = self.now;
        let mut seed = local.octets().to_vec();
        seed.extend_from_slice(&peer.octets());
        seed.extend_from_slice(&now.to_be_bytes());
        let icookie: [u8; 8] = ike::filler(&seed, 8).try_into().unwrap_or([1; 8]);
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.phase = IkePhase::Main(1);
        sa.initiator = true;
        sa.icookie = icookie;
        sa.rcookie = [0; 8];
        sa.started_us = now;
        let mut transforms: Vec<Transform> = d.crypto.policies.iter().take(16).zip(1u8..).map(|(p, n)| policy_transform(n, Some(p))).collect();
        if transforms.is_empty() {
            transforms.push(policy_transform(1, None));
        }
        let proposal = Proposal { number: 1, protocol: ike::PROTO_ISAKMP, spi: Vec::new(), transforms };
        let m = Isakmp::clear(icookie, [0; 8], ike::EXCH_MAIN, 0, vec![Payload::Sa(vec![proposal])]);
        if self.state.get(&d.id).is_some_and(|s| s.debug_packet) {
            self.log(d.id, &format!("IKE: starting main mode with {peer} for traffic a crypto map protects"));
        }
        self.ike_send(t, d, local, peer, &m);
    }

    /// Records a failed exchange and logs why.
    fn ike_fail(&mut self, d: &Device, peer: Ipv4Addr, why: String) {
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.phase = IkePhase::Failed;
        sa.failed = Some(why.clone());
        self.log(d.id, &format!("IKE with {peer} failed: {why}"));
    }

    /// An ISAKMP message to this router: the next step of the exchange it belongs to.
    pub(crate) fn ike_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, payload: &[u8]) -> Result<(), String> {
        let m = Isakmp::decode(payload).map_err(|e| e.to_string())?;
        let (peer, local) = (ip.src, ip.dst);
        if d.crypto.applied.is_empty() && d.crypto.key_for(peer).is_none() {
            return Err(format!("ISAKMP from {peer}: IPsec is not configured here"));
        }
        let other = owner(t, d, peer);
        let now = self.now;
        let cur = self.state.get(&d.id).and_then(|s| s.ipsec.get(&peer)).cloned().unwrap_or_default();
        let my_key = d.crypto.key_for(peer).unwrap_or_default().to_string();
        // The crypto map entry towards this peer, on the interface it reached.
        let my_entry = crate::l3_ifaces(d)
            .into_iter()
            .find(|i| i.net.addr == local)
            .and_then(|i| d.crypto.applied.iter().find(|(n, _)| same_name(n, &i.name)).map(|(_, m)| m.clone()))
            .and_then(|m| d.crypto.map(&m).find(|e| e.peer == Some(peer)).cloned());
        if m.exchange == ike::EXCH_INFO {
            let what = m.notify().map_or("notification", ike::notify_name);
            if cur.phase == IkePhase::Established || cur.icookie != m.icookie {
                return Err(format!("ISAKMP {what} from {peer} for no exchange in progress"));
            }
            // The reason, from both configurations; the notification says where it stopped.
            let why = match (&my_entry, other) {
                (Some(e), Some(_)) => negotiate(t, d, local, e).err(),
                _ => None,
            }
            .unwrap_or_else(|| format!("the peer refused ({what})"));
            let at = match cur.phase {
                IkePhase::Main(n) => format!("main mode message {}", n + 1),
                IkePhase::Quick(n) => format!("quick mode message {}", n + 1),
                _ => "the exchange".into(),
            };
            self.ike_fail(d, peer, format!("{why}; the peer sent {what} in place of {at}"));
            return Ok(());
        }
        let reply_to =
            |m: &Isakmp, rcookie: [u8; 8], exch: u8, id: u32, payloads: Vec<Payload>| Isakmp::clear(m.icookie, rcookie, exch, id, payloads);
        let refuse = |m: &Isakmp, rcookie: [u8; 8], kind: u16| {
            Isakmp::clear(m.icookie, rcookie, ike::EXCH_INFO, 1, vec![Payload::Notify { protocol: ike::PROTO_ISAKMP, kind, spi: Vec::new() }])
        };
        if m.exchange == ike::EXCH_MAIN && m.rcookie == [0; 8] {
            // Main mode message 1: a new exchange, this router responding.
            let mut seed = m.icookie.to_vec();
            seed.extend_from_slice(&local.octets());
            seed.extend_from_slice(&now.to_be_bytes());
            let rcookie: [u8; 8] = ike::filler(&seed, 8).try_into().unwrap_or([2; 8]);
            let st = self.st(d.id);
            if !st.ipsec.contains_key(&peer) && st.ipsec.len() >= MAX_SAS {
                st.ipsec.clear();
            }
            let sa = st.ipsec.entry(peer).or_default();
            *sa = IpsecSa { local, peer, phase: IkePhase::Main(2), icookie: m.icookie, rcookie, started_us: now, ..IpsecSa::default() };
            if let Some(e) = &my_entry {
                sa.map = e.map.clone();
                sa.seq = e.seq;
            }
            if let Some(i) = crate::l3_ifaces(d).into_iter().find(|i| i.net.addr == local) {
                sa.iface = i.name;
            }
            let checked = match other {
                Some(o) => policy_check(o, d),
                None => Ok(()),
            };
            if let Err(why) = checked {
                self.ike_fail(d, peer, format!("{why}; refused main mode message 1"));
                self.ike_send(t, d, local, peer, &refuse(&m, rcookie, ike::NO_PROPOSAL_CHOSEN));
                return Ok(());
            }
            let chosen = other
                .and_then(|o| o.crypto.policies.iter().find(|x| d.crypto.policies.iter().any(|y| x.agrees(y))))
                .or_else(|| d.crypto.policies.first());
            let proposal = Proposal { number: 1, protocol: ike::PROTO_ISAKMP, spi: Vec::new(), transforms: vec![policy_transform(1, chosen)] };
            self.ike_send(t, d, local, peer, &reply_to(&m, rcookie, ike::EXCH_MAIN, 0, vec![Payload::Sa(vec![proposal])]));
            return Ok(());
        }
        if m.icookie != cur.icookie || (cur.rcookie != [0; 8] && m.rcookie != cur.rcookie) {
            return Err(format!("ISAKMP from {peer} for an exchange this router doesn't know (cookies)"));
        }
        let ke = |who: &[u8]| {
            let mut seed = who.to_vec();
            seed.extend_from_slice(&m.icookie);
            vec![Payload::Ke(ike::filler(&seed, 128)), Payload::Nonce(ike::filler(&[seed.as_slice(), &b"n"[..]].concat(), 16))]
        };
        let hash = |what: &[u8]| Payload::Hash(ike::filler(&[my_key.as_bytes(), &m.icookie[..], what].concat(), 20));
        let rc = cur.rcookie;
        match (m.exchange, cur.initiator, cur.phase) {
            // Initiator: the policy was accepted (message 2): keys and nonces (message 3).
            (ike::EXCH_MAIN, true, IkePhase::Main(1)) if m.proposal().is_some() => {
                let sa = self.st(d.id).ipsec.entry(peer).or_default();
                sa.rcookie = m.rcookie;
                sa.phase = IkePhase::Main(3);
                let out = Isakmp::clear(m.icookie, m.rcookie, ike::EXCH_MAIN, 0, ke(&local.octets()));
                self.ike_send(t, d, local, peer, &out);
            }
            // Responder: message 3 in, message 4 out.
            (ike::EXCH_MAIN, false, IkePhase::Main(2)) => {
                self.st(d.id).ipsec.entry(peer).or_default().phase = IkePhase::Main(4);
                self.ike_send(t, d, local, peer, &reply_to(&m, rc, ike::EXCH_MAIN, 0, ke(&local.octets())));
            }
            // Initiator: message 4 in, the encrypted identity (message 5) out.
            (ike::EXCH_MAIN, true, IkePhase::Main(3)) => {
                self.st(d.id).ipsec.entry(peer).or_default().phase = IkePhase::Main(5);
                let out = Isakmp::sealed(m.icookie, rc, ike::EXCH_MAIN, 0, &[id_payload(local), hash(b"I")], my_key.as_bytes());
                self.ike_send(t, d, local, peer, &out);
            }
            // Responder: message 5 must open with this router's key and the keys must agree.
            (ike::EXCH_MAIN, false, IkePhase::Main(4)) => {
                let verdict = match other {
                    Some(o) => auth_check(o, peer, d, local),
                    None => Ok(()),
                }
                .and_then(|()| {
                    m.open(my_key.as_bytes()).map(|_| ()).map_err(|_| "main mode message 5 does not decrypt with this router's key".to_string())
                });
                if let Err(why) = verdict {
                    self.ike_fail(d, peer, format!("{why}; main mode message 5 failed authentication"));
                    self.ike_send(t, d, local, peer, &refuse(&m, rc, ike::AUTHENTICATION_FAILED));
                    return Ok(());
                }
                self.st(d.id).ipsec.entry(peer).or_default().phase = IkePhase::Main(6);
                let out = Isakmp::sealed(m.icookie, rc, ike::EXCH_MAIN, 0, &[id_payload(local), hash(b"R")], my_key.as_bytes());
                self.ike_send(t, d, local, peer, &out);
            }
            // Initiator: main mode done (message 6); quick mode message 1 proposes ESP.
            (ike::EXCH_MAIN, true, IkePhase::Main(5)) => {
                if m.open(my_key.as_bytes()).is_err() {
                    self.ike_fail(d, peer, "main mode message 6 does not decrypt with this router's key".into());
                    return Ok(());
                }
                let qm_id = u32::from_be_bytes(ike::filler(&[&m.icookie[..], &b"qm"[..]].concat(), 4).try_into().unwrap_or([0, 0, 0, 1])) | 1;
                let sa = self.st(d.id).ipsec.entry(peer).or_default();
                sa.phase = IkePhase::Quick(1);
                sa.qm_id = qm_id;
                let ts = my_entry.as_ref().and_then(|e| e.transform_set.as_deref()).and_then(|n| d.crypto.transform_set(n));
                let spi_in = d.crypto.key_for(peer).map_or(0, |k| spi(k, peer, local));
                let proposal =
                    Proposal { number: 1, protocol: ike::PROTO_ESP, spi: spi_in.to_be_bytes().to_vec(), transforms: vec![esp_transform(ts)] };
                let payloads =
                    [hash(b"Q1"), Payload::Sa(vec![proposal]), Payload::Nonce(ike::filler(&m.icookie, 16)), id_payload(local), id_payload(peer)];
                let out = Isakmp::sealed(m.icookie, rc, ike::EXCH_QUICK, qm_id, &payloads, my_key.as_bytes());
                self.ike_send(t, d, local, peer, &out);
            }
            // Responder: quick mode message 1 must match a crypto map entry here.
            (ike::EXCH_QUICK, false, IkePhase::Main(6)) => {
                let verdict = match (other, &my_entry) {
                    (Some(o), _) => {
                        let oe = crate::l3_ifaces(o)
                            .into_iter()
                            .find(|i| i.net.addr == peer)
                            .and_then(|i| o.crypto.applied.iter().find(|(n, _)| same_name(n, &i.name)).map(|(_, m)| m.clone()))
                            .and_then(|mname| o.crypto.map(&mname).find(|e| e.peer == Some(local)).cloned());
                        match oe {
                            Some(oe) => quick_check(o, peer, &oe, d, local),
                            None => Err(format!("no crypto map entry here for {peer}")),
                        }
                    }
                    (None, Some(_)) => Ok(()),
                    (None, None) => Err(format!("no crypto map entry here for {peer}")),
                };
                if let Err(why) = verdict {
                    self.ike_fail(d, peer, format!("{why}; refused quick mode message 1"));
                    self.ike_send(t, d, local, peer, &refuse(&m, rc, ike::NO_PROPOSAL_CHOSEN));
                    return Ok(());
                }
                self.st(d.id).ipsec.entry(peer).or_default().phase = IkePhase::Quick(2);
                let ts = my_entry.as_ref().and_then(|e| e.transform_set.as_deref()).and_then(|n| d.crypto.transform_set(n));
                let proposal = Proposal {
                    number: 1,
                    protocol: ike::PROTO_ESP,
                    spi: spi(&my_key, peer, local).to_be_bytes().to_vec(),
                    transforms: vec![esp_transform(ts)],
                };
                let payloads =
                    [hash(b"Q2"), Payload::Sa(vec![proposal]), Payload::Nonce(ike::filler(&m.rcookie, 16)), id_payload(peer), id_payload(local)];
                let out = Isakmp::sealed(m.icookie, rc, ike::EXCH_QUICK, m.message_id, &payloads, my_key.as_bytes());
                self.ike_send(t, d, local, peer, &out);
            }
            // Initiator: quick mode message 2 accepted the transform: up, and message 3 confirms.
            (ike::EXCH_QUICK, true, IkePhase::Quick(1)) => {
                let out = Isakmp::sealed(m.icookie, rc, ike::EXCH_QUICK, m.message_id, &[hash(b"Q3")], my_key.as_bytes());
                self.ike_up(d, local, peer, &my_key);
                self.ike_send(t, d, local, peer, &out);
            }
            // Responder: quick mode message 3: up.
            (ike::EXCH_QUICK, false, IkePhase::Quick(2)) => self.ike_up(d, local, peer, &my_key),
            (_, _, phase) => {
                return Err(format!("ISAKMP from {peer} not expected now ({})", phase.describe()));
            }
        }
        Ok(())
    }

    /// The exchange completed: the security association is up.
    fn ike_up(&mut self, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, key: &str) {
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.phase = IkePhase::Established;
        sa.failed = None;
        sa.spi_out = spi(key, local, peer);
        sa.spi_in = spi(key, peer, local);
        self.log(d.id, &format!("IKE with {peer}: main mode and quick mode complete, IPsec security association up"));
    }

    /// An ESP packet addressed to this router: checked, opened, and the inner packet handled as
    /// if it had arrived on `ifc`.
    pub(crate) fn esp_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4) -> Result<(), String> {
        let esp = Esp::decode(&ip.payload).map_err(|e| e.to_string())?;
        let expects = d.crypto.applied.values().any(|m| d.crypto.map(m).any(|e| e.peer == Some(ip.src)));
        if expects && !self.state.get(&d.id).and_then(|s| s.ipsec.get(&ip.src)).is_some_and(|sa| sa.phase == IkePhase::Established) {
            if let Some(sa) = self.st(d.id).ipsec.get_mut(&ip.src) {
                sa.drops = sa.drops.saturating_add(1);
            }
            return Err(format!("ESP from {} before IKE set up a security association", ip.src));
        }
        let key = d.crypto.key_for(ip.src).map(str::to_string);
        let (Some(key), true) = (key, expects) else {
            return Err(format!("ESP from {}, which is not an IPsec peer here", ip.src));
        };
        let opened = if esp.spi != spi(&key, ip.src, ip.dst) {
            Err(format!("ESP SPI 0x{:08x} from {} matches no security association", esp.spi, ip.src))
        } else {
            match esp.open(key.as_bytes()) {
                Ok((NEXT_IPV4, inner)) => Ok(inner),
                Ok((next, _)) => Err(format!("ESP carrying next header {next} is not understood")),
                Err(_) => Err(format!("ESP from {} failed its integrity check (the keys differ?)", ip.src)),
            }
        };
        let st = self.st(d.id);
        if !st.ipsec.contains_key(&ip.src) && st.ipsec.len() >= MAX_SAS {
            st.ipsec.clear();
        }
        let sa = st.ipsec.entry(ip.src).or_default();
        sa.peer = ip.src;
        sa.local = ip.dst;
        match opened {
            Ok(inner) => {
                sa.decaps = sa.decaps.saturating_add(1);
                self.ip_in(t, d, ifc, &inner)
            }
            Err(e) => {
                sa.drops = sa.drops.saturating_add(1);
                Err(e)
            }
        }
    }

    /// Every IPsec peer this router has protected traffic with (or tried to).
    pub fn ipsec_sas(&self, d: packetcraft_core::DeviceId) -> Vec<IpsecSa> {
        self.state.get(&d).map(|s| s.ipsec.values().cloned().collect()).unwrap_or_default()
    }

    /// Forgets the security associations and their counters (`clear crypto sa`).
    pub fn clear_ipsec(&mut self, d: packetcraft_core::DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.ipsec.clear();
        }
    }
}
