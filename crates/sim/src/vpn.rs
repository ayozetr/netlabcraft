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
//! would find out, each router deciding from its own configuration and what it received: the
//! responder picks an offered policy one of its own matches (or refuses at main mode message
//! 2); messages 3 and 4 carry real Diffie-Hellman values and nonces; SKEYID and its children
//! come from the pre-shared key and the shared secret (RFC 2409 §5), so pre-shared keys that
//! differ leave message 5 undecryptable and its HASH_I wrong (AUTHENTICATION-FAILED); quick
//! mode, encrypted and hashed with those keys, is refused at message 2 when the transform sets
//! or crypto map entries don't match. The refusing side sends a notification (in the clear
//! here, so the event list shows it). AES, DES and 3DES policies encrypt for real
//! (`crate::ikev1`). ESP keys come from quick mode's KEYMAT; `esp-aes` (any key size),
//! `esp-des`, `esp-3des` and `esp-null` with HMAC integrity run for real, AH with the stand-in
//! cipher of `netlabcraft_wire::vpn`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;

use netlabcraft_core::MacAddr;
use netlabcraft_model::vpn::{CryptoMapEntry, Tunnel, TunnelSource};
use netlabcraft_model::{Device, DeviceKind, End, NextHop, Topology, ifname};
use netlabcraft_wire::ah::PROTO_AH;
use netlabcraft_wire::ike::{self, Isakmp, Payload, Proposal, Transform};
use netlabcraft_wire::vpn::{Gre, NEXT_IPV4, PROTO_ESP, PROTO_GRE};
use netlabcraft_wire::{ETHERTYPE_IPV4, Ipv4, PROTO_UDP, Udp};

use crate::ikev1::{P1Suite, V1State};
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
    port.checked_sub(TUNNEL_BASE).and_then(|n| u32::try_from(n).ok()).filter(|n| *n <= netlabcraft_model::vpn::MAX_TUNNEL_NUMBER)
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
    if let (Some(x), Some(y)) = (netlabcraft_model::vpn::tunnel_number(a), netlabcraft_model::vpn::tunnel_number(b)) {
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
    /// An IKEv2 exchange's state (`None` for IKEv1).
    pub v2: Option<Box<crate::ikev2::V2State>>,
    /// An IKEv1 exchange's keys and state (`None` for IKEv2).
    pub v1: Option<Box<V1State>>,
    /// When the IPsec SA (its current keys) came up, simulated µs.
    pub up_us: u64,
    /// Its lifetime: seconds and kilobytes.
    pub life: (u32, u32),
    /// Bytes it protected since it came up, both ways.
    pub bytes: u64,
    /// Times its keys were renewed since IKE first set it up.
    pub rekeys: u32,
    /// When the rekey under way started (0: none).
    pub rekey_us: u64,
    /// The inbound SA before the last rekey, still accepted until the peer deletes it: SPI,
    /// algorithms and key.
    pub prev_in: Option<(u32, Option<netlabcraft_wire::vpn::EspSuite>, Vec<u8>)>,
}

/// Where an IPsec SA stands in its lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Life {
    Fresh,
    /// Past 90 % of its time or volume: time to renew the keys.
    Soft,
    Expired,
}

impl IpsecSa {
    /// Seconds and kilobytes left.
    pub fn remaining(&self, now: u64) -> (u64, u64) {
        let secs = u64::from(self.life.0).saturating_sub(now.saturating_sub(self.up_us) / 1_000_000);
        let kb = u64::from(self.life.1).saturating_sub(self.bytes / 1024);
        (secs, kb)
    }

    pub fn life_state(&self, now: u64) -> Life {
        if self.life == (0, 0) {
            return Life::Fresh;
        }
        let used_us = now.saturating_sub(self.up_us);
        let life_us = u64::from(self.life.0).saturating_mul(1_000_000);
        let life_b = u64::from(self.life.1).saturating_mul(1024);
        if used_us >= life_us || self.bytes >= life_b {
            Life::Expired
        } else if used_us >= life_us / 10 * 9 || self.bytes >= life_b / 10 * 9 {
            Life::Soft
        } else {
            Life::Fresh
        }
    }

    /// New keys are in: the old inbound SA is kept for packets already on their way.
    pub(crate) fn rekeyed(&mut self, now: u64, life: (u32, u32), first: bool) {
        if !first {
            self.rekeys = self.rekeys.saturating_add(1);
        }
        self.up_us = now;
        self.life = life;
        self.bytes = 0;
        self.rekey_us = 0;
    }
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
    /// IKEv2: IKE_SA_INIT sent or answered.
    V2Init,
    /// IKEv2: IKE_AUTH sent.
    V2Auth,
}

impl IkePhase {
    pub fn describe(self) -> String {
        match self {
            IkePhase::Idle => "idle".into(),
            IkePhase::Main(n) => format!("main mode, message {n}"),
            IkePhase::Quick(n) => format!("quick mode, message {n}"),
            IkePhase::Established => "established".into(),
            IkePhase::Failed => "failed".into(),
            IkePhase::V2Init => "IKEv2 IKE_SA_INIT".into(),
            IkePhase::V2Auth => "IKEv2 IKE_AUTH".into(),
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
            v2: None,
            v1: None,
            up_us: 0,
            life: (0, 0),
            bytes: 0,
            rekeys: 0,
            rekey_us: 0,
            prev_in: None,
        }
    }
}

/// An exchange that hasn't finished (or failed) this long is started again by the next packet.
const IKE_RETRY_US: u64 = 2_000_000;

/// Most IPsec peers tracked per router.
const MAX_SAS: usize = 256;

/// An SPI this router picks for its inbound SA, from a seed: never below 256.
fn fresh_spi(seed: &[u8]) -> u32 {
    let h = netlabcraft_wire::md5::md5(seed);
    u32::from_be_bytes([h[0], h[1], h[2], h[3]]) | 0x100
}

fn spi_u32(spi: &[u8]) -> u32 {
    spi.get(..4).and_then(|s| s.try_into().ok()).map_or(0, u32::from_be_bytes)
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

/// Whether both routers' configuration still agrees on the association a crypto map entry asks
/// for: the checks of every IKE stage in the order the exchange makes them (to explain a
/// refusal, and to notice a configuration changed under an association that is up).
fn negotiate(t: &Topology, d: &Device, local: Ipv4Addr, e: &CryptoMapEntry) -> Result<(), String> {
    let (peer, _) = local_check(d, e)?;
    let p = owner(t, d, peer).ok_or_else(|| format!("no device has the address {peer}"))?;
    policy_check(d, p)?;
    auth_check(d, local, p, peer)?;
    quick_check(d, local, e, p, peer)
}

/// ISAKMP attribute values for a policy (RFC 2409 appendix A).
fn policy_transform(number: u8, p: Option<&netlabcraft_model::vpn::IsakmpPolicy>) -> Transform {
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

/// The integrity algorithm of a transform set made only of AH transforms (`ah-sha-hmac`,
/// `ah-md5-hmac`, `ah-sha256-hmac`: the last one wins), the case that runs as a real AH
/// association. `None` for anything else, including AH next to ESP transforms in one set.
pub(crate) fn ah_integ(ts: &netlabcraft_model::vpn::TransformSet) -> Option<netlabcraft_wire::suite::Integ> {
    use netlabcraft_wire::suite::Integ;
    let mut integ = None;
    for t in &ts.transforms {
        integ = Some(match t.split_whitespace().next()? {
            "ah-sha-hmac" => Integ::HmacSha1_96,
            "ah-md5-hmac" => Integ::HmacMd5_96,
            "ah-sha256-hmac" => Integ::HmacSha256_128,
            _ => return None,
        });
    }
    integ
}

/// The security protocol a quick-mode or child-SA exchange carries for a suite.
pub(crate) fn proto_of(suite: Option<netlabcraft_wire::vpn::EspSuite>) -> u8 {
    if suite.is_some_and(|s| s.encr == netlabcraft_wire::vpn::EspEncr::Ah) { ike::PROTO_AH } else { ike::PROTO_ESP }
}

/// The proposal of quick mode for a transform set: the protocol (AH for a set of AH transforms
/// only, ESP otherwise) and its transform (RFC 2407 §4.4.3, §4.4.4 and §4.5).
fn child_proposal(ts: Option<&netlabcraft_model::vpn::TransformSet>) -> (u8, Transform) {
    use netlabcraft_wire::suite::Integ;
    let mode = if ts.is_some_and(|t| t.transport) { 2 } else { 1 };
    if let Some(integ) = ts.and_then(ah_integ) {
        // AH_MD5 = 2, AH_SHA = 3, AH_SHA2_256 = 5; the authentication algorithm attribute
        // names the same HMAC (HMAC-MD5 = 1, HMAC-SHA = 2, HMAC-SHA2-256 = 5).
        let (id, auth) = match integ {
            Integ::HmacMd5_96 => (2, 1),
            Integ::HmacSha256_128 => (5, 5),
            _ => (3, 2),
        };
        return (ike::PROTO_AH, Transform { number: 1, id, attrs: vec![(1, 1), (2, 3_600), (4, mode), (5, auth)] });
    }
    (ike::PROTO_ESP, esp_transform(ts))
}

/// ESP transform from a transform set (RFC 2407 §4.4.4 and §4.5).
fn esp_transform(ts: Option<&netlabcraft_model::vpn::TransformSet>) -> Transform {
    let has = |w: &str| ts.is_some_and(|t| t.transforms.iter().any(|x| x.split_whitespace().next() == Some(w)));
    let bits: u16 = ts.and_then(|t| t.transforms.iter().find_map(|x| x.strip_prefix("esp-aes ").and_then(|b| b.parse().ok()))).unwrap_or(128);
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
    } else if has("esp-sha256-hmac") {
        attrs.push((5, 5));
    } else if has("esp-md5-hmac") {
        attrs.push((5, 1));
    }
    if id == 12 {
        attrs.push((6, bits));
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

/// The IPsec algorithms a transform set asks for, when every one of them is implemented: ESP
/// (AES-CBC of any key size, DES-CBC, 3DES-CBC or NULL, with HMAC-SHA1-96, HMAC-SHA-256-128,
/// HMAC-MD5-96 or no integrity check) or AH on its own (HMAC-SHA1-96, HMAC-MD5-96 or
/// HMAC-SHA-256-128). `None` for AH next to ESP in one set or a set with neither cipher nor
/// integrity: those keep the stand-in cipher of `netlabcraft_wire::vpn`.
pub(crate) fn esp_suite(ts: &netlabcraft_model::vpn::TransformSet) -> Option<netlabcraft_wire::vpn::EspSuite> {
    use netlabcraft_wire::suite::Integ;
    use netlabcraft_wire::vpn::{EspEncr, EspSuite};
    if let Some(integ) = ah_integ(ts) {
        return Some(EspSuite { encr: EspEncr::Ah, integ });
    }
    let mut encr = None;
    let mut integ = Integ::None;
    for t in &ts.transforms {
        let mut w = t.split_whitespace();
        match w.next()? {
            "esp-aes" => {
                let bits: u16 = w.next().map_or(Some(128), |b| b.parse().ok())?;
                if ![128, 192, 256].contains(&bits) {
                    return None;
                }
                encr = Some(EspEncr::AesCbc(bits));
            }
            "esp-des" => encr = Some(EspEncr::Des),
            "esp-3des" => encr = Some(EspEncr::Des3),
            "esp-null" => encr = Some(EspEncr::Null),
            "esp-sha-hmac" => integ = Integ::HmacSha1_96,
            "esp-sha256-hmac" => integ = Integ::HmacSha256_128,
            "esp-md5-hmac" => integ = Integ::HmacMd5_96,
            _ => return None,
        }
    }
    let encr = encr?;
    if encr == EspEncr::Null && integ == Integ::None {
        return None;
    }
    Some(EspSuite { encr, integ })
}

/// Whether an access list permits a packet (an explicit permit; a missing list permits
/// nothing).
pub(crate) fn acl_permits(d: &Device, name: &str, ip: &Ipv4) -> bool {
    let Some(acl) = d.acls.iter().find(|a| a.name == name) else { return false };
    acl.evaluate(&crate::acl_key(ip)).0
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
        // The outer header copies the inner packet's "don't fragment" (so `ip mtu` on the way can
        // fragment GRE carrying packets that may be fragmented).
        netlabcraft_wire::ipfrag::set_dont_fragment(&mut outer, netlabcraft_wire::ipfrag::dont_fragment(&gre.payload));
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
        if ip.protocol == PROTO_ESP || ip.protocol == PROTO_AH || is_isakmp(&ip) {
            return Ok(None);
        }
        let Some(entry) = d.crypto.map(&map).find(|e| e.acl.as_deref().is_some_and(|a| acl_permits(d, a, &ip))).cloned() else { return Ok(None) };
        if entry.ikev2_profile.is_some() {
            return self.crypto_out_v2(t, d, out, datagram, &entry);
        }
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
        // Established: renew the keys near the end of the lifetime, start over after it.
        match sa.life_state(now) {
            Life::Expired => {
                let why = format!("IPsec SA with {peer} reached the end of its lifetime; negotiating new keys");
                *sa = IpsecSa {
                    local,
                    peer,
                    iface: out.name.clone(),
                    map: entry.map.clone(),
                    seq: entry.seq,
                    drops: sa.drops.saturating_add(1),
                    ..IpsecSa::default()
                };
                self.log(d.id, &why);
                self.ike_start(t, d, local, peer);
                return Err(format!("IPsec: {why}"));
            }
            Life::Soft if sa.initiator && (sa.rekey_us == 0 || now.saturating_sub(sa.rekey_us) >= IKE_RETRY_US) => {
                self.ike_rekey_v1(t, d, local, peer);
            }
            _ => {}
        }
        // The configuration may have changed since.
        match negotiate(t, d, local, &entry) {
            Err(why) => {
                let sa = self.st(d.id).ipsec.entry(peer).or_default();
                sa.failed = Some(why.clone());
                sa.phase = IkePhase::Failed;
                sa.drops = sa.drops.saturating_add(1);
                Err(format!("IPsec: no security association with {peer}: {why}"))
            }
            Ok(()) => {
                let sa = self.st(d.id).ipsec.entry(peer).or_default();
                sa.failed = None;
                sa.encaps = sa.encaps.saturating_add(1);
                sa.bytes = sa.bytes.saturating_add(u64::try_from(datagram.len()).unwrap_or(u64::MAX));
                let seq = u32::try_from(sa.encaps).unwrap_or(u32::MAX);
                let (suite, key) = sa.v1.as_deref().map(|v| (v.esp_suite, v.esp_out.clone())).unwrap_or_default();
                let spi = sa.spi_out;
                let id = self.ip_id();
                Ok(Some((crate::ikev2::ipsec_seal(suite, &key, spi, seq, id, local, peer, datagram), peer)))
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
        let mut transforms: Vec<Transform> = d.crypto.policies.iter().take(16).zip(1u8..).map(|(p, n)| policy_transform(n, Some(p))).collect();
        if transforms.is_empty() {
            transforms.push(policy_transform(1, None));
        }
        let proposal = Proposal { number: 1, protocol: ike::PROTO_ISAKMP, spi: Vec::new(), transforms };
        let sa_payload = Payload::Sa(vec![proposal]);
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.phase = IkePhase::Main(1);
        sa.initiator = true;
        sa.icookie = icookie;
        sa.rcookie = [0; 8];
        sa.started_us = now;
        sa.v1 = Some(Box::new(V1State::new(None, sa_payload.body())));
        let m = Isakmp::clear(icookie, [0; 8], ike::EXCH_MAIN, 0, vec![sa_payload]);
        if self.state.get(&d.id).is_some_and(|s| s.debug_packet) {
            self.log(d.id, &format!("IKE: starting main mode with {peer} for traffic a crypto map protects"));
        }
        self.ike_send(t, d, local, peer, &m);
    }

    /// Records a failed exchange and logs why. A failed rekey leaves the SA it would have
    /// renewed as it is.
    fn ike_fail(&mut self, d: &Device, peer: Ipv4Addr, why: String) {
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        if sa.phase == IkePhase::Established {
            sa.rekey_us = 0;
            self.log(d.id, &format!("IKE with {peer}: renewing the IPsec keys failed: {why}"));
            return;
        }
        sa.phase = IkePhase::Failed;
        sa.failed = Some(why.clone());
        self.log(d.id, &format!("IKE with {peer} failed: {why}"));
    }

    /// Keeps the exchange state; a quick mode under an SA that is up (a rekey) leaves it up.
    fn set_v1(&mut self, d: &Device, peer: Ipv4Addr, v1: V1State, phase: IkePhase) {
        let now = self.now;
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.v1 = Some(Box::new(v1));
        if sa.phase == IkePhase::Established {
            if sa.rekey_us == 0 {
                sa.rekey_us = now.max(1);
            }
        } else {
            sa.phase = phase;
        }
    }

    /// Renews the IPsec keys: a new quick mode under the ISAKMP SA that is up.
    fn ike_rekey_v1(&mut self, t: &Topology, d: &Device, local: Ipv4Addr, peer: Ipv4Addr) {
        let now = self.now;
        let Some(sa) = self.state.get(&d.id).and_then(|s| s.ipsec.get(&peer)).cloned() else { return };
        let Some(v1) = sa.v1.as_deref().cloned() else { return };
        let entry = crate::ikev2::entry_for(d, local, peer);
        let seed = [&sa.icookie[..], &sa.rcookie[..], &local.octets()[..], &now.to_be_bytes()[..], b"rekey"].concat();
        self.log(d.id, &format!("IKE with {peer}: the IPsec SA nears the end of its lifetime; quick mode renews its keys"));
        if let Some(s) = self.st(d.id).ipsec.get_mut(&peer) {
            s.rekey_us = now.max(1);
        }
        self.qm1(t, d, local, peer, v1, &seed, &entry, sa.icookie, sa.rcookie);
    }

    /// Quick mode message 1: the ESP proposal from the crypto map entry, HASH(1) over it.
    #[allow(clippy::too_many_arguments)]
    fn qm1(
        &mut self,
        t: &Topology,
        d: &Device,
        local: Ipv4Addr,
        peer: Ipv4Addr,
        mut v1: V1State,
        seed: &[u8],
        my_entry: &Option<CryptoMapEntry>,
        ic: [u8; 8],
        rc: [u8; 8],
    ) {
        let qm_id = u32::from_be_bytes(ike::filler(&[seed, &b"qm"[..]].concat(), 4).try_into().unwrap_or([0, 0, 0, 1])) | 1;
        let ts = my_entry.as_ref().and_then(|e| e.transform_set.as_deref()).and_then(|n| d.crypto.transform_set(n));
        v1.spi_mine = fresh_spi(&[seed, b"spi-i"].concat());
        v1.esp_suite = ts.and_then(esp_suite);
        v1.qm_ni = ike::filler(&[seed, b"qm-ni"].concat(), 16);
        v1.start_qm(qm_id);
        let (protocol, transform) = child_proposal(ts);
        let proposal = Proposal { number: 1, protocol, spi: v1.spi_mine.to_be_bytes().to_vec(), transforms: vec![transform] };
        let rest = vec![Payload::Sa(vec![proposal]), Payload::Nonce(v1.qm_ni.clone()), id_payload(local), id_payload(peer)];
        let mut payloads = vec![Payload::Hash(v1.hash_qm(qm_id, None, &rest))];
        payloads.extend(rest);
        let out = v1.seal(ic, rc, ike::EXCH_QUICK, qm_id, &payloads, true);
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.qm_id = qm_id;
        self.set_v1(d, peer, v1, IkePhase::Quick(1));
        self.ike_send(t, d, local, peer, &out);
    }

    /// An ISAKMP message to this router: the next step of the exchange it belongs to.
    pub(crate) fn ike_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, payload: &[u8]) -> Result<(), String> {
        if payload.get(17).is_some_and(|v| v >> 4 == 2) {
            return self.ike2_in(t, d, ip, payload);
        }
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
        let refuse = |m: &Isakmp, rcookie: [u8; 8], kind: u16| {
            Isakmp::clear(m.icookie, rcookie, ike::EXCH_INFO, 1, vec![Payload::Notify { protocol: ike::PROTO_ISAKMP, kind, spi: Vec::new() }])
        };
        if m.exchange == ike::EXCH_MAIN && m.rcookie == [0; 8] {
            return self.ike_main1(t, d, local, peer, &m, &my_entry, other);
        }
        if m.icookie != cur.icookie || (cur.rcookie != [0; 8] && m.rcookie != cur.rcookie) {
            return Err(format!("ISAKMP from {peer} for an exchange this router doesn't know (cookies)"));
        }
        let mut v1 = cur.v1.as_deref().cloned().unwrap_or_default();
        let (ic, rc) = (m.icookie, if cur.rcookie == [0; 8] { m.rcookie } else { cur.rcookie });
        let seed = [&ic[..], &rc[..], &local.octets()[..], &now.to_be_bytes()[..]].concat();
        let ke_nonce = |ke: Vec<u8>, n: Vec<u8>| vec![Payload::Ke(ke), Payload::Nonce(n)];
        let take_ke = |m: &Isakmp| {
            let ke = m.payloads.iter().find_map(|p| if let Payload::Ke(k) = p { Some(k.clone()) } else { None });
            let n = m.payloads.iter().find_map(|p| if let Payload::Nonce(k) = p { Some(k.clone()) } else { None });
            ke.zip(n)
        };
        // Quick mode under an SA that is up renews its keys: the steps are those of the first.
        let phase = match (cur.phase, m.exchange) {
            (IkePhase::Established, ike::EXCH_QUICK) if cur.rekey_us != 0 && m.message_id == cur.qm_id => {
                if cur.initiator {
                    IkePhase::Quick(1)
                } else {
                    IkePhase::Quick(2)
                }
            }
            (IkePhase::Established, ike::EXCH_QUICK) if !cur.initiator => IkePhase::Main(6),
            (p, _) => p,
        };
        match (m.exchange, cur.initiator, phase) {
            // Initiator: the responder chose a policy (message 2): keys and nonces (message 3).
            (ike::EXCH_MAIN, true, IkePhase::Main(1)) if m.proposal().is_some() => {
                let chosen = m.proposal().and_then(|p| p.transforms.first()).cloned();
                let offered: Vec<Transform> = d.crypto.policies.iter().map(|p| policy_transform(0, Some(p))).collect();
                let ok = chosen.as_ref().filter(|c| offered.is_empty() || offered.iter().any(|o| crate::ikev1::same_policy(o, c)));
                let Some(chosen) = ok else {
                    self.ike_fail(d, peer, "the peer chose an ISAKMP policy this router did not offer".into());
                    return Ok(());
                };
                v1.suite = Some(P1Suite::from_transform(chosen));
                v1.ke_i = v1.make_ke(&seed);
                v1.ni = ike::filler(&[&seed[..], b"ni"].concat(), 16);
                let out = Isakmp::clear(ic, m.rcookie, ike::EXCH_MAIN, 0, ke_nonce(v1.ke_i.clone(), v1.ni.clone()));
                let sa = self.st(d.id).ipsec.entry(peer).or_default();
                sa.rcookie = m.rcookie;
                self.set_v1(d, peer, v1, IkePhase::Main(3));
                self.ike_send(t, d, local, peer, &out);
            }
            // Responder: message 3 in (the initiator's value and nonce), message 4 out; both
            // ends can now work out the keys.
            (ike::EXCH_MAIN, false, IkePhase::Main(2)) => {
                let Some((ke, ni)) = take_ke(&m) else {
                    self.ike_fail(d, peer, "main mode message 3 lacks the key exchange or the nonce".into());
                    return Ok(());
                };
                v1.ke_i = ke;
                v1.ni = ni;
                v1.ke_r = v1.make_ke(&seed);
                v1.nr = ike::filler(&[&seed[..], b"nr"].concat(), 16);
                if !v1.derive(my_key.as_bytes(), &ic, &rc, false) {
                    self.ike_fail(d, peer, "the peer's Diffie-Hellman value is not valid for the group".into());
                    self.ike_send(t, d, local, peer, &refuse(&m, rc, ike::INVALID_KEY_INFORMATION));
                    return Ok(());
                }
                let out = Isakmp::clear(ic, rc, ike::EXCH_MAIN, 0, ke_nonce(v1.ke_r.clone(), v1.nr.clone()));
                self.set_v1(d, peer, v1, IkePhase::Main(4));
                self.ike_send(t, d, local, peer, &out);
            }
            // Initiator: message 4 in, the encrypted identity and HASH_I (message 5) out.
            (ike::EXCH_MAIN, true, IkePhase::Main(3)) => {
                let Some((ke, nr)) = take_ke(&m) else {
                    self.ike_fail(d, peer, "main mode message 4 lacks the key exchange or the nonce".into());
                    return Ok(());
                };
                v1.ke_r = ke;
                v1.nr = nr;
                if !v1.derive(my_key.as_bytes(), &ic, &rc, true) {
                    self.ike_fail(d, peer, "the peer's Diffie-Hellman value is not valid for the group".into());
                    return Ok(());
                }
                let id = id_payload(local);
                let hash = v1.hash_p1(true, &ic, &rc, &id.body());
                let out = v1.seal(ic, rc, ike::EXCH_MAIN, 0, &[id, Payload::Hash(hash)], false);
                self.set_v1(d, peer, v1, IkePhase::Main(5));
                self.ike_send(t, d, local, peer, &out);
            }
            // Responder: message 5 must decrypt with the keys and carry the right HASH_I.
            (ike::EXCH_MAIN, false, IkePhase::Main(4)) => {
                let opened = v1.open(&m, false);
                let verdict = opened.and_then(|list| {
                    let id = list.iter().find(|p| matches!(p, Payload::Id { .. })).map(Payload::body).ok_or(crate::ikev1::OPEN_FAILED)?;
                    let want = v1.hash_p1(true, &ic, &rc, &id);
                    if crate::ikev1::hash_of(&list) == Some(&want[..]) { Ok(()) } else { Err(crate::ikev1::OPEN_FAILED.to_string()) }
                });
                if let Err(e) = verdict {
                    // The configurations say why, when the peer is known.
                    let why = other.and_then(|o| auth_check(o, peer, d, local).err()).unwrap_or_else(|| format!("main mode message 5 {e}"));
                    self.ike_fail(d, peer, format!("{why}; main mode message 5 failed authentication"));
                    self.ike_send(t, d, local, peer, &refuse(&m, rc, ike::AUTHENTICATION_FAILED));
                    return Ok(());
                }
                let id = id_payload(local);
                let hash = v1.hash_p1(false, &ic, &rc, &id.body());
                let out = v1.seal(ic, rc, ike::EXCH_MAIN, 0, &[id, Payload::Hash(hash)], false);
                self.set_v1(d, peer, v1, IkePhase::Main(6));
                self.ike_send(t, d, local, peer, &out);
            }
            // Initiator: main mode done (message 6, HASH_R checked); quick mode message 1
            // proposes ESP.
            (ike::EXCH_MAIN, true, IkePhase::Main(5)) => {
                let ok = v1.open(&m, false).is_ok_and(|list| {
                    list.iter()
                        .find(|p| matches!(p, Payload::Id { .. }))
                        .is_some_and(|id| crate::ikev1::hash_of(&list) == Some(&v1.hash_p1(false, &ic, &rc, &id.body())[..]))
                });
                if !ok {
                    self.ike_fail(d, peer, format!("main mode message 6 {}", crate::ikev1::OPEN_FAILED));
                    return Ok(());
                }
                self.qm1(t, d, local, peer, v1, &seed, &my_entry, ic, rc);
            }
            // Responder: quick mode message 1 must authenticate and match a crypto map entry
            // here with the same transforms.
            (ike::EXCH_QUICK, false, IkePhase::Main(6)) => {
                v1.start_qm(m.message_id);
                let list = match v1.open(&m, true) {
                    Ok(l) if crate::ikev1::hash_of(&l) == Some(&v1.hash_qm(m.message_id, None, crate::ikev1::after_hash(&l))[..]) => l,
                    _ => {
                        self.ike_fail(d, peer, format!("quick mode message 1 {}", crate::ikev1::OPEN_FAILED));
                        return Ok(());
                    }
                };
                let offer = list.iter().find_map(|p| if let Payload::Sa(v) = p { v.first().cloned() } else { None });
                let ts = my_entry.as_ref().and_then(|e| e.transform_set.as_deref()).and_then(|n| d.crypto.transform_set(n));
                let verdict = match (&my_entry, &offer) {
                    (None, _) => Err(format!("no crypto map entry here for {peer}")),
                    (_, None) => Err("quick mode message 1 carries no ESP proposal".to_string()),
                    (Some(_), Some(o)) if (o.protocol, o.transforms.first()) != (child_proposal(ts).0, Some(&child_proposal(ts).1)) => {
                        // The configurations say how they differ, when the peer is known.
                        let why = other.and_then(|o| {
                            let oe = crate::l3_ifaces(o)
                                .into_iter()
                                .find(|i| i.net.addr == peer)
                                .and_then(|i| o.crypto.applied.iter().find(|(n, _)| same_name(n, &i.name)).map(|(_, m)| m.clone()))
                                .and_then(|mname| o.crypto.map(&mname).find(|e| e.peer == Some(local)).cloned())?;
                            quick_check(o, peer, &oe, d, local).err()
                        });
                        Err(why.unwrap_or_else(|| "the peer's ESP transforms differ from this router's transform set".to_string()))
                    }
                    _ => Ok(()),
                };
                if let Err(why) = verdict {
                    self.ike_fail(d, peer, format!("{why}; refused quick mode message 1"));
                    self.ike_send(t, d, local, peer, &refuse(&m, rc, ike::NO_PROPOSAL_CHOSEN));
                    return Ok(());
                }
                v1.qm_ni = list.iter().find_map(|p| if let Payload::Nonce(n) = p { Some(n.clone()) } else { None }).unwrap_or_default();
                v1.spi_peer = offer.as_ref().map_or(0, |o| spi_u32(&o.spi));
                v1.spi_mine = fresh_spi(&[&seed[..], b"spi-r"].concat());
                v1.qm_nr = ike::filler(&[&seed[..], b"qm-nr"].concat(), 16);
                v1.esp_suite = ts.and_then(esp_suite);
                let (protocol, transform) = child_proposal(ts);
                let proposal = Proposal { number: 1, protocol, spi: v1.spi_mine.to_be_bytes().to_vec(), transforms: vec![transform] };
                let rest = vec![Payload::Sa(vec![proposal]), Payload::Nonce(v1.qm_nr.clone()), id_payload(peer), id_payload(local)];
                let mut payloads = vec![Payload::Hash(v1.hash_qm(m.message_id, Some(&v1.qm_ni), &rest))];
                payloads.extend(rest);
                let out = v1.seal(ic, rc, ike::EXCH_QUICK, m.message_id, &payloads, true);
                let sa = self.st(d.id).ipsec.entry(peer).or_default();
                sa.qm_id = m.message_id;
                self.set_v1(d, peer, v1, IkePhase::Quick(2));
                self.ike_send(t, d, local, peer, &out);
            }
            // Initiator: quick mode message 2 accepted the transform: up, and message 3 confirms.
            (ike::EXCH_QUICK, true, IkePhase::Quick(1)) => {
                let ni = v1.qm_ni.clone();
                let list = match v1.open(&m, true) {
                    Ok(l) if crate::ikev1::hash_of(&l) == Some(&v1.hash_qm(m.message_id, Some(&ni), crate::ikev1::after_hash(&l))[..]) => l,
                    _ => {
                        self.ike_fail(d, peer, format!("quick mode message 2 {}", crate::ikev1::OPEN_FAILED));
                        return Ok(());
                    }
                };
                v1.qm_nr = list.iter().find_map(|p| if let Payload::Nonce(n) = p { Some(n.clone()) } else { None }).unwrap_or_default();
                v1.spi_peer = list.iter().find_map(|p| if let Payload::Sa(v) = p { v.first().map(|x| spi_u32(&x.spi)) } else { None }).unwrap_or(0);
                let out = v1.seal(ic, rc, ike::EXCH_QUICK, m.message_id, &[Payload::Hash(v1.hash_qm3(m.message_id))], true);
                self.ike_up(d, local, peer, v1);
                self.ike_send(t, d, local, peer, &out);
            }
            // Responder: quick mode message 3 (HASH(3) checked): up.
            (ike::EXCH_QUICK, false, IkePhase::Quick(2)) => {
                let ok = v1.open(&m, true).is_ok_and(|l| crate::ikev1::hash_of(&l) == Some(&v1.hash_qm3(m.message_id)[..]));
                if !ok {
                    self.ike_fail(d, peer, format!("quick mode message 3 {}", crate::ikev1::OPEN_FAILED));
                    return Ok(());
                }
                self.ike_up(d, local, peer, v1);
            }
            (_, _, phase) => {
                return Err(format!("ISAKMP from {peer} not expected now ({})", phase.describe()));
            }
        }
        Ok(())
    }

    /// Main mode message 1: a new exchange, this router responding with the first offered
    /// policy that one of its own policies matches.
    #[allow(clippy::too_many_arguments)]
    fn ike_main1(
        &mut self,
        t: &Topology,
        d: &Device,
        local: Ipv4Addr,
        peer: Ipv4Addr,
        m: &Isakmp,
        my_entry: &Option<CryptoMapEntry>,
        other: Option<&Device>,
    ) -> Result<(), String> {
        let now = self.now;
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
        if let Some(e) = my_entry {
            sa.map = e.map.clone();
            sa.seq = e.seq;
        }
        if let Some(i) = crate::l3_ifaces(d).into_iter().find(|i| i.net.addr == local) {
            sa.iface = i.name;
        }
        let offered: Vec<Transform> = m.proposal().map(|p| p.transforms.clone()).unwrap_or_default();
        let mine: Vec<(Transform, bool)> =
            d.crypto.policies.iter().map(|p| (policy_transform(0, Some(p)), p.authentication == "pre-share")).collect();
        let chosen = if mine.is_empty() {
            offered.iter().find(|t| crate::ikev1::pre_shared(t)).cloned()
        } else {
            offered.iter().find(|t| mine.iter().any(|(x, psk)| *psk && crate::ikev1::same_policy(x, t))).cloned()
        };
        let Some(chosen) = chosen else {
            let why = other.and_then(|o| policy_check(o, d).err()).unwrap_or_else(|| "no ISAKMP policy matches the peer's".to_string());
            self.ike_fail(d, peer, format!("{why}; refused main mode message 1"));
            let n = Isakmp::clear(
                m.icookie,
                rcookie,
                ike::EXCH_INFO,
                1,
                vec![Payload::Notify { protocol: ike::PROTO_ISAKMP, kind: ike::NO_PROPOSAL_CHOSEN, spi: Vec::new() }],
            );
            self.ike_send(t, d, local, peer, &n);
            return Ok(());
        };
        let sa_b = m.payloads.iter().find(|p| matches!(p, Payload::Sa(_))).map(Payload::body).unwrap_or_default();
        let v1 = V1State::new(Some(P1Suite::from_transform(&chosen)), sa_b);
        self.set_v1(d, peer, v1, IkePhase::Main(2));
        let proposal = Proposal { number: 1, protocol: ike::PROTO_ISAKMP, spi: Vec::new(), transforms: vec![chosen] };
        self.ike_send(t, d, local, peer, &Isakmp::clear(m.icookie, rcookie, ike::EXCH_MAIN, 0, vec![Payload::Sa(vec![proposal])]));
        Ok(())
    }

    /// The exchange completed: the security association is up, with ESP keys from KEYMAT.
    fn ike_up(&mut self, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, mut v1: V1State) {
        let len = v1.esp_suite.map_or(36, netlabcraft_wire::vpn::EspSuite::keymat_len);
        v1.esp_in = v1.keymat(v1.spi_mine, len);
        v1.esp_out = v1.keymat(v1.spi_peer, len);
        let now = self.now;
        let life = crate::ikev2::entry_for(d, local, peer).map_or((0, 0), |e| d.crypto.lifetime(&e));
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        let first = sa.phase != IkePhase::Established;
        if !first {
            sa.prev_in = sa.v1.as_deref().map(|old| (sa.spi_in, old.esp_suite, old.esp_in.clone()));
        }
        sa.rekeyed(now, life, first);
        sa.phase = IkePhase::Established;
        sa.failed = None;
        sa.local = local;
        sa.spi_out = v1.spi_peer;
        sa.spi_in = v1.spi_mine;
        sa.v1 = Some(Box::new(v1));
        self.log(d.id, &format!("IKE with {peer}: main mode and quick mode complete, IPsec security association up"));
    }

    /// An ESP packet addressed to this router: checked, opened, and the inner packet handled as
    /// if it had arrived on `ifc`.
    pub(crate) fn esp_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4) -> Result<(), String> {
        let spi = crate::ikev2::inbound_spi(ip).map_err(|e| e.to_string())?;
        if let Some(r) = self.esp_in_v2(t, d, ifc, ip, spi) {
            return r;
        }
        let expects = d.crypto.applied.values().any(|m| d.crypto.map(m).any(|e| e.peer == Some(ip.src)));
        let sa = self.state.get(&d.id).and_then(|s| s.ipsec.get(&ip.src));
        let keys = sa.filter(|sa| sa.phase == IkePhase::Established).and_then(|sa| {
            let prev = sa.prev_in.clone().filter(|p| p.0 == spi);
            sa.v1.as_deref().map(|v| prev.unwrap_or((sa.spi_in, v.esp_suite, v.esp_in.clone())))
        });
        let Some((spi_in, suite, key)) = keys else {
            if let Some(sa) = self.st(d.id).ipsec.get_mut(&ip.src) {
                sa.drops = sa.drops.saturating_add(1);
            }
            return Err(if expects {
                format!("ESP from {} before IKE set up a security association", ip.src)
            } else {
                format!("ESP from {}, which is not an IPsec peer here", ip.src)
            });
        };
        let opened = if spi != spi_in {
            Err(format!("IPsec SPI 0x{spi:08x} from {} matches no security association", ip.src))
        } else {
            match crate::ikev2::ipsec_open(suite, &key, ip) {
                Ok((NEXT_IPV4, inner)) => Ok(inner),
                Ok((next, _)) => Err(format!("ESP carrying next header {next} is not understood")),
                Err(_) => Err(format!("ESP from {} failed its integrity check (the keys differ?)", ip.src)),
            }
        };
        let sa = self.st(d.id).ipsec.entry(ip.src).or_default();
        match opened {
            Ok(inner) => {
                sa.decaps = sa.decaps.saturating_add(1);
                sa.bytes = sa.bytes.saturating_add(u64::try_from(inner.len()).unwrap_or(u64::MAX));
                self.ip_in(t, d, ifc, &inner)
            }
            Err(e) => {
                sa.drops = sa.drops.saturating_add(1);
                Err(e)
            }
        }
    }

    /// Every IPsec peer this router has protected traffic with (or tried to).
    pub fn ipsec_sas(&self, d: netlabcraft_core::DeviceId) -> Vec<IpsecSa> {
        self.state.get(&d).map(|s| s.ipsec.values().cloned().collect()).unwrap_or_default()
    }

    /// Forgets the security associations and their counters (`clear crypto sa`).
    pub fn clear_ipsec(&mut self, d: netlabcraft_core::DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.ipsec.clear();
        }
    }
}
