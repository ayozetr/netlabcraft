//! Site-to-site VPNs: GRE tunnels (RFC 2784) and basic IPsec ESP (RFC 4303).
//!
//! A tunnel is a layer-3 interface without a port. Its line protocol is up when it is not shut
//! down, its source names an up interface (or an address of one) and its destination is
//! reachable through a connected or static route over a physical interface (dynamic routes are
//! not consulted, so a tunnel never depends on routes learned through itself). Packets routed
//! out of it get an outer IPv4 header (source → destination, protocol 47) and a GRE header,
//! and are routed again; a packet whose outer destination would itself go into a tunnel is
//! dropped (recursive routing), so nothing is encapsulated twice. The far router accepts GRE
//! for an up tunnel whose source and destination mirror the packet's addresses and routes the
//! inner packet as if it had arrived on that tunnel. Routing protocols see the two tunnel ends
//! as a point-to-point link.
//!
//! IPsec: a crypto map applied to an interface protects the packets leaving it that its access
//! list permits. The security association is decided from both routers' configuration (the
//! ISAKMP exchange itself is not simulated): matching pre-shared keys, transform sets and, if
//! both have them, an agreeing ISAKMP policy. Protected packets travel as ESP whose cipher is a
//! keyed XOR, **not real encryption** (see `packetcraft_wire::vpn`).

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use packetcraft_core::MacAddr;
use packetcraft_model::vpn::{CryptoMapEntry, Tunnel, TunnelSource};
use packetcraft_model::{Device, DeviceKind, End, NextHop, Topology, ifname};
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

/// A tunnel's endpoints (source, destination) when its line protocol is up, else why not.
/// `phys` are the router's physical up interfaces.
pub fn tunnel_state(d: &Device, tun: &Tunnel, phys: &[L3If]) -> Result<(Ipv4Addr, Ipv4Addr), &'static str> {
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
    if !phys_routable(d, dst, phys) {
        return Err("no route to the tunnel destination");
    }
    if tun.ip.is_some_and(|n| n.contains(dst)) {
        return Err("the destination is inside the tunnel's own network");
    }
    Ok((src, dst))
}

/// The tunnels of a router that are up, given its physical up interfaces.
pub(crate) fn up_tunnels(d: &Device, phys: &[L3If]) -> Vec<L3If> {
    tunnel_ifaces(d)
        .into_iter()
        .filter(|i| tunnel_of(i.port).and_then(|n| d.tunnels.iter().find(|t| t.number == n)).is_some_and(|t| tunnel_state(d, t, phys).is_ok()))
        .collect()
}

/// `show interfaces tunnel N`: the tunnel's endpoints when up, else why its line protocol is
/// down.
pub fn tunnel_status(t: &Topology, d: &Device, number: u32) -> Result<(Ipv4Addr, Ipv4Addr), String> {
    let tun = d.tunnels.iter().find(|x| x.number == number).ok_or_else(|| format!("no interface Tunnel{number}"))?;
    let phys = crate::phys_up_ifaces(t, d);
    tunnel_state(d, tun, &phys).map_err(str::to_string)
}

/// The far ends of an up tunnel interface: every router tunnel, up, whose source and
/// destination mirror this one's. Reported as (device, tunnel pseudo port).
pub(crate) fn tunnel_peers(t: &Topology, d: &Device, ifc: &L3If) -> BTreeSet<End> {
    let mut out = BTreeSet::new();
    let Some(tun) = tunnel_of(ifc.port).and_then(|n| d.tunnels.iter().find(|x| x.number == n)) else { return out };
    let Ok((src, dst)) = tunnel_state(d, tun, &crate::phys_up_ifaces(t, d)) else { return out };
    for b in t.devices.iter().filter(|b| b.id != d.id && b.kind == DeviceKind::Router && !b.tunnels.is_empty()) {
        let phys = crate::phys_up_ifaces(t, b);
        for bt in b.tunnels.iter().filter(|x| x.ip.is_some()) {
            if tunnel_state(b, bt, &phys) == Ok((dst, src)) {
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
    /// Why the association could not be set up, when it couldn't.
    pub failed: Option<String>,
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
        }
    }
}

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

/// Decides the association a crypto map entry asks for, from both routers' configuration.
fn negotiate(t: &Topology, d: &Device, local: Ipv4Addr, e: &CryptoMapEntry) -> Result<Agreed, String> {
    let peer = e.peer.ok_or_else(|| format!("crypto map {} {} has no peer (`set peer`)", e.map, e.seq))?;
    let ts_name = e.transform_set.as_deref().ok_or_else(|| format!("crypto map {} {} has no transform set", e.map, e.seq))?;
    let ts = d.crypto.transform_set(ts_name).ok_or_else(|| format!("transform set {ts_name} does not exist"))?;
    let key = d.crypto.key_for(peer).ok_or_else(|| format!("no ISAKMP key for peer {peer}"))?;
    let p = t
        .devices
        .iter()
        .find(|x| x.id != d.id && crate::l3_ifaces(x).iter().any(|i| i.net.addr == peer))
        .ok_or_else(|| format!("no device has the address {peer}"))?;
    let p_if = crate::l3_ifaces(p).into_iter().find(|i| i.net.addr == peer).map(|i| i.name).unwrap_or_default();
    let p_map =
        p.crypto.applied.iter().find(|(n, _)| same_name(n, &p_if)).map(|(_, m)| m).ok_or_else(|| format!("the peer has no crypto map on {p_if}"))?;
    let pe = p.crypto.map(p_map).find(|x| x.peer == Some(local)).ok_or_else(|| format!("the peer's crypto map {p_map} has no entry for {local}"))?;
    let p_ts =
        pe.transform_set.as_deref().and_then(|n| p.crypto.transform_set(n)).ok_or("the peer's crypto map entry has no usable transform set")?;
    if p_ts.transforms != ts.transforms || p_ts.transport != ts.transport {
        return Err(format!("transform sets differ ({} here, {} at the peer)", ts.transforms.join(" "), p_ts.transforms.join(" ")));
    }
    if !d.crypto.policies.is_empty() && !p.crypto.policies.is_empty() {
        let agreed = d.crypto.policies.iter().find(|a| p.crypto.policies.iter().any(|b| a.agrees(b))).ok_or("no ISAKMP policy matches the peer's")?;
        if agreed.authentication != "pre-share" {
            return Err("the matching ISAKMP policy does not use pre-shared keys".into());
        }
    }
    let p_key = p.crypto.key_for(local).ok_or_else(|| format!("the peer has no ISAKMP key for {local}"))?;
    if p_key != key {
        return Err("the pre-shared keys differ (ISAKMP authentication failed)".into());
    }
    Ok(Agreed { key: key.to_string(), spi_out: spi(key, local, peer), spi_in: spi(key, peer, local) })
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
        let (src, dst) = tunnel_state(d, tun, &crate::phys_up_ifaces(t, d)).map_err(|e| format!("{} is down: {e}", tun.name()))?;
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
                    .is_some_and(|x| tunnel_state(d, x, &phys) == Ok((ip.dst, ip.src)))
            })
            .ok_or_else(|| format!("GRE from {}: no tunnel here is up with that destination and source {}", ip.src, ip.dst))?;
        self.ip_in(t, d, &ifc, &g.payload)
    }

    /// IPsec on the way out of `out`: the ESP packet and the peer to route it to when a crypto
    /// map protects the datagram, `None` when it leaves in the clear.
    pub(crate) fn crypto_out(&mut self, t: &Topology, d: &Device, out: &L3If, datagram: &[u8]) -> Result<Option<(Vec<u8>, Ipv4Addr)>, String> {
        let Some(map) = d.crypto.applied.iter().find(|(n, _)| same_name(n, &out.name)).map(|(_, m)| m.clone()) else { return Ok(None) };
        let Ok(ip) = Ipv4::decode(datagram) else { return Ok(None) };
        if ip.protocol == PROTO_ESP {
            return Ok(None);
        }
        let Some(entry) = d.crypto.map(&map).find(|e| e.acl.as_deref().is_some_and(|a| acl_permits(d, a, &ip))).cloned() else { return Ok(None) };
        let peer = entry.peer.unwrap_or(Ipv4Addr::UNSPECIFIED);
        let local = out.net.addr;
        let result = negotiate(t, d, local, &entry);
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
        match result {
            Err(why) => {
                sa.failed = Some(why.clone());
                sa.drops = sa.drops.saturating_add(1);
                Err(format!("IPsec: no security association with {peer}: {why}"))
            }
            Ok(a) => {
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

    /// An ESP packet addressed to this router: checked, opened, and the inner packet handled as
    /// if it had arrived on `ifc`.
    pub(crate) fn esp_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4) -> Result<(), String> {
        let esp = Esp::decode(&ip.payload).map_err(|e| e.to_string())?;
        let expects = d.crypto.applied.values().any(|m| d.crypto.map(m).any(|e| e.peer == Some(ip.src)));
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
