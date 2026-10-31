//! IP MTU: datagrams larger than an interface's IP MTU (`ip mtu`, else 1500 bytes on Ethernet
//! and serial interfaces and 1476 on GRE tunnels) are fragmented (RFC 791), or, when they say
//! "don't fragment", dropped with an ICMP "fragmentation needed" carrying the MTU back to their
//! source (RFC 1191). Fragments addressed to a device are reassembled there (see `ip_in`);
//! routers forward them as they are. On a tunnel the MTU applies to the inner packet, before
//! encapsulation; ESP copies the inner packet's "don't fragment" and, when the encrypted packet
//! would be too large with it set, tells the source the MTU left after the IPsec overhead.
//!
//! Path MTU discovery (RFC 1191 §6): a device told "fragmentation needed" remembers the MTU
//! towards that destination for ten minutes and sends a TCP segment the message quotes again
//! in pieces that fit; TCP senders keep their segments within the MSS the peer's SYN announced
//! and the path MTU.

use std::collections::{BTreeMap, VecDeque};
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::ipfrag::{dont_fragment, frag_needed, fragment, tcp_mss};
use netlabcraft_wire::tcp::{self, Tcp};
use netlabcraft_wire::{Icmp, Ipv4, PROTO_ICMP};

use crate::{L3If, Sim, l3_ifaces};

impl Sim {
    /// Whether an encoded datagram's destination is one of this device's addresses (or a
    /// broadcast), so its fragments are reassembled here.
    pub(crate) fn addressed_here(d: &Device, datagram: &[u8]) -> bool {
        let Some(b) = datagram.get(16..20) else { return false };
        let dst = Ipv4Addr::new(b[0], b[1], b[2], b[3]);
        dst == Ipv4Addr::BROADCAST || l3_ifaces(d).iter().any(|i| i.net.addr == dst || (i.net.prefix < 31 && i.net.broadcast() == dst))
    }

    /// Sends a datagram too large for `out`'s MTU: in fragments, or not at all when DF is set.
    pub(crate) fn transmit_mtu(&mut self, t: &Topology, d: &Device, out: &L3If, nh: Ipv4Addr, datagram: Vec<u8>, mtu: u16) -> Result<(), String> {
        if dont_fragment(&datagram) {
            self.bump(d.id, "dropped: fragmentation needed and DF set");
            let Ok(orig) = Ipv4::decode(&datagram) else { return Err("an unreadable datagram is too large for the MTU".into()) };
            // Never an error about an ICMP error.
            let is_error = orig.protocol == PROTO_ICMP && !matches!(orig.payload.first(), Some(0 | 8));
            if !is_error {
                let msg = frag_needed(mtu, &Icmp::quote(&datagram));
                let mine = l3_ifaces(d).iter().any(|i| i.net.addr == orig.src);
                if mine {
                    // Raised here (a ping from this router): told to itself.
                    let err = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_ICMP, src: out.net.addr, dst: orig.src, payload: msg };
                    let _ = self.local_in(t, d, out, err);
                } else if let Ok(src) = self.source_for(t, d, orig.src) {
                    let err = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_ICMP, src, dst: orig.src, payload: msg };
                    let _ = self.send_ip(t, d, err);
                }
            }
            return Err(format!("{} bytes need fragmenting for {} (IP MTU {mtu}) but \"don't fragment\" is set", datagram.len(), out.name));
        }
        let pieces = fragment(&datagram, usize::from(mtu)).map_err(|e| e.to_string())?;
        self.bump(d.id, "fragmented");
        for p in pieces {
            self.transmit(t, d, out, nh, p)?;
        }
        Ok(())
    }
}

/// The IP MTU of an Ethernet or serial interface when `ip mtu` doesn't set one.
pub const DEFAULT_MTU: u16 = 1_500;
/// A GRE tunnel's: 1500 less the outer IPv4 header (20) and the GRE header (4).
pub const TUNNEL_MTU: u16 = 1_476;
/// How long a learnt path MTU is kept (RFC 1191 §6.3 suggests ten minutes).
const PMTU_AGE_US: u64 = 600 * 1_000_000;
/// Most destinations, peers and sent segments remembered per device.
const MAX_PMTU: usize = 256;
const MAX_SENT: usize = 64;
/// The largest TCP payload a 1500-byte MTU carries (and what a peer is assumed to take).
pub const DEFAULT_MSS: u16 = 1_460;

/// The IP MTU of an interface: `ip mtu`, else the default of its kind.
pub(crate) fn ip_mtu(d: &Device, out: &L3If) -> u16 {
    d.security.ip_mtu.get(&out.name).copied().unwrap_or(if crate::vpn::is_tunnel(out.port) { TUNNEL_MTU } else { DEFAULT_MTU })
}

/// What a device remembers for path MTU discovery.
#[derive(Debug, Clone, Default)]
pub(crate) struct Pmtu {
    /// Destination → (path MTU, when learnt).
    paths: BTreeMap<Ipv4Addr, (u16, u64)>,
    /// (peer, peer port) → the MSS its SYN announced.
    peer_mss: BTreeMap<(Ipv4Addr, u16), u16>,
    /// TCP segments with data sent lately: (source, destination, segment), for sending them
    /// again in smaller pieces when the path turns out narrower.
    sent: VecDeque<(Ipv4Addr, Ipv4Addr, Tcp)>,
}

impl Sim {
    /// The path MTU known towards `dst` (`None`: nothing learnt, or it aged out).
    pub fn path_mtu(&self, d: DeviceId, dst: Ipv4Addr) -> Option<u16> {
        let now = self.now;
        self.state.get(&d).and_then(|s| s.pmtu.paths.get(&dst)).filter(|(_, at)| now.saturating_sub(*at) < PMTU_AGE_US).map(|(m, _)| *m)
    }

    /// The largest TCP payload to send to `dst`:`port`: no more than its SYN announced, nor
    /// than the path MTU allows.
    pub(crate) fn send_mss(&self, d: DeviceId, dst: Ipv4Addr, port: u16) -> usize {
        let peer = self.state.get(&d).and_then(|s| s.pmtu.peer_mss.get(&(dst, port)).copied()).unwrap_or(DEFAULT_MSS);
        let path = self.path_mtu(d, dst).map_or(DEFAULT_MSS, |m| m.saturating_sub(40));
        usize::from(peer.min(path).max(8))
    }

    /// A SYN arrived: remembers the MSS it announces.
    pub(crate) fn note_peer_mss(&mut self, d: DeviceId, ip: &Ipv4) {
        let Some(mss) = tcp_mss(&ip.payload) else { return };
        let Some(port) = ip.payload.get(..2).map(|b| u16::from_be_bytes([b[0], b[1]])) else { return };
        let p = &mut self.st(d).pmtu;
        if p.peer_mss.len() >= MAX_PMTU {
            p.peer_mss.pop_first();
        }
        p.peer_mss.insert((ip.src, port), mss.max(64));
    }

    /// A TCP segment with data goes out: kept for a while in case the path is narrower.
    pub(crate) fn note_sent(&mut self, d: DeviceId, src: Ipv4Addr, dst: Ipv4Addr, seg: &Tcp) {
        if seg.payload.is_empty() {
            return;
        }
        let p = &mut self.st(d).pmtu;
        if p.sent.len() >= MAX_SENT {
            p.sent.pop_front();
        }
        p.sent.push_back((src, dst, seg.clone()));
    }

    /// "Fragmentation needed" arrived for a datagram this device sent (RFC 1191 §6): the path
    /// MTU towards its destination is learnt, and a TCP segment it quotes is sent again in
    /// pieces that fit.
    pub(crate) fn pmtu_learn(&mut self, t: &Topology, d: &Device, icmp: &[u8]) {
        let (Some(&3), Some(&netlabcraft_wire::ipfrag::UNREACH_FRAG_NEEDED)) = (icmp.first(), icmp.get(1)) else { return };
        let original = icmp.get(8..).unwrap_or_default();
        let Some(dst) = original.get(16..20).map(|b| Ipv4Addr::new(b[0], b[1], b[2], b[3])) else { return };
        let hl = usize::from(original.first().map_or(5, |b| b & 0x0f)) * 4;
        // The next-hop MTU; a router that says 0 (RFC 1191 §5): one step down the plateaus.
        let said = icmp.get(6..8).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
        let total = original.get(2..4).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
        let mtu = if said >= 68 { said } else { [1492, 1006, 508, 296, 68].into_iter().find(|p| *p < total).unwrap_or(68) };
        let now = self.now;
        let p = &mut self.st(d.id).pmtu;
        let known = p.paths.get(&dst).map_or(u16::MAX, |(m, _)| *m);
        if p.paths.len() >= MAX_PMTU && !p.paths.contains_key(&dst) {
            p.paths.pop_first();
        }
        p.paths.insert(dst, (mtu.min(known).max(68), now));
        if original.get(9).copied() != Some(netlabcraft_wire::PROTO_TCP) {
            return;
        }
        let port = |at: usize| original.get(hl + at..hl + at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
        let seq = original.get(hl + 4..hl + 8).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
        let (Some(sp), Some(dp), Some(seq)) = (port(0), port(2), seq) else { return };
        let Some(at) = p.sent.iter().position(|(_, to, s)| *to == dst && s.src_port == sp && s.dst_port == dp && s.seq == seq) else { return };
        let Some((src, dst, seg)) = p.sent.remove(at) else { return };
        if self.state.get(&d.id).is_some_and(|s| s.debug_packet) {
            self.log(d.id, &format!("IP debug: the path MTU to {dst} is {mtu}; a TCP segment goes again in smaller pieces"));
        }
        let piece = usize::from(mtu.saturating_sub(40).max(8));
        let chunks: Vec<&[u8]> = seg.payload.chunks(piece).collect();
        let last = chunks.len().saturating_sub(1);
        let mut s = seg.seq;
        for (i, c) in chunks.into_iter().enumerate() {
            let flags = if i == last { seg.flags } else { seg.flags & !(tcp::FIN | tcp::PSH) };
            let one = Tcp { seq: s, flags, payload: c.to_vec(), ..seg.clone() };
            s = s.wrapping_add(u32::try_from(c.len()).unwrap_or(0));
            let _ = self.seg_to(t, d, src, dst, one);
        }
    }

    /// An ESP packet for `datagram` leaving by `out`: it copies the inner packet's "don't
    /// fragment"; when that is set and the ESP packet is too large, the inner packet's source
    /// is told the MTU left for it after the IPsec overhead (and the packet dropped).
    pub(crate) fn esp_fits(&mut self, t: &Topology, d: &Device, out: &L3If, datagram: &[u8], esp: &mut [u8]) -> Result<(), String> {
        let df = dont_fragment(datagram);
        netlabcraft_wire::ipfrag::set_dont_fragment(esp, df);
        let mtu = ip_mtu(d, out);
        if !df || esp.len() <= usize::from(mtu) {
            return Ok(());
        }
        let overhead = esp.len().saturating_sub(datagram.len());
        let room = u16::try_from(usize::from(mtu).saturating_sub(overhead)).unwrap_or(68).max(68);
        self.bump(d.id, "dropped: fragmentation needed and DF set");
        if let Ok(orig) = Ipv4::decode(datagram)
            && !l3_ifaces(d).iter().any(|i| i.net.addr == orig.src)
            && let Ok(src) = self.source_for(t, d, orig.src)
        {
            let msg = frag_needed(room, &Icmp::quote(datagram));
            let err = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_ICMP, src, dst: orig.src, payload: msg };
            let _ = self.send_ip(t, d, err);
        }
        Err(format!("{} bytes with IPsec need fragmenting for {} (IP MTU {mtu}) but \"don't fragment\" is set", esp.len(), out.name))
    }
}
