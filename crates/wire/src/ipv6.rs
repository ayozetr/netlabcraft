//! IPv6 (RFC 8200), ICMPv6 (RFC 4443) and Neighbor Discovery (RFC 4861).

use std::net::Ipv6Addr;

use packetcraft_core::MacAddr;

use crate::{Result, WireError, be16, checksum, mac_at};

pub const ETHERTYPE_IPV6: u16 = 0x86dd;
pub const PROTO_ICMPV6: u8 = 58;

/// All nodes on the link (`ff02::1`).
pub const ALL_NODES: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);
/// All routers on the link (`ff02::2`).
pub const ALL_ROUTERS: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 2);

/// The solicited-node multicast group of an address (`ff02::1:ffXX:XXXX`).
pub fn solicited_node(a: Ipv6Addr) -> Ipv6Addr {
    let low = u128::from(a) & 0x00ff_ffff;
    Ipv6Addr::from(u128::from(Ipv6Addr::new(0xff02, 0, 0, 0, 0, 1, 0xff00, 0)) | low)
}

/// The Ethernet group address of an IPv6 multicast group (`33:33` + its last 32 bits).
pub fn multicast_mac(a: Ipv6Addr) -> MacAddr {
    let o = a.octets();
    MacAddr([0x33, 0x33, o[12], o[13], o[14], o[15]])
}

fn ip6_at(b: &[u8], at: usize) -> Option<Ipv6Addr> {
    let s: [u8; 16] = b.get(at..at + 16)?.try_into().ok()?;
    Some(Ipv6Addr::from(s))
}

/// An IPv6 packet without extension headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ipv6 {
    pub hop_limit: u8,
    pub next_header: u8,
    pub src: Ipv6Addr,
    pub dst: Ipv6Addr,
    pub payload: Vec<u8>,
}

impl Ipv6 {
    pub fn encode(&self) -> Vec<u8> {
        let len = u16::try_from(self.payload.len()).unwrap_or(u16::MAX);
        let mut out = vec![0x60, 0, 0, 0];
        out.extend_from_slice(&len.to_be_bytes());
        out.push(self.next_header);
        out.push(self.hop_limit);
        out.extend_from_slice(&self.src.octets());
        out.extend_from_slice(&self.dst.octets());
        out.extend_from_slice(self.payload.get(..usize::from(len)).unwrap_or(&self.payload));
        out
    }

    pub fn decode(b: &[u8]) -> Result<Ipv6> {
        let t = || WireError::Truncated("ipv6");
        let v = *b.first().ok_or_else(t)?;
        if v >> 4 != 6 {
            return Err(WireError::Unsupported("ipv6", format!("version {}", v >> 4)));
        }
        let len = usize::from(be16(b, 4).ok_or_else(t)?);
        Ok(Ipv6 {
            next_header: *b.get(6).ok_or_else(t)?,
            hop_limit: *b.get(7).ok_or_else(t)?,
            src: ip6_at(b, 8).ok_or_else(t)?,
            dst: ip6_at(b, 24).ok_or_else(t)?,
            payload: b.get(40..40 + len).ok_or_else(t)?.to_vec(),
        })
    }
}

/// A prefix a router advertises for address autoconfiguration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixInfo {
    pub prefix: Ipv6Addr,
    pub len: u8,
}

/// The ICMPv6 messages the simulator speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icmp6 {
    /// Type 1. `original` is the start of the offending packet.
    Unreachable {
        code: u8,
        original: Vec<u8>,
    },
    /// Type 3, code 0 (hop limit exceeded in transit).
    TimeExceeded {
        original: Vec<u8>,
    },
    EchoRequest {
        id: u16,
        seq: u16,
        data: Vec<u8>,
    },
    EchoReply {
        id: u16,
        seq: u16,
        data: Vec<u8>,
    },
    /// Type 133.
    RouterSolicit {
        mac: Option<MacAddr>,
    },
    /// Type 134. `lifetime` 0: not a default router.
    RouterAdvert {
        hop_limit: u8,
        lifetime: u16,
        mac: Option<MacAddr>,
        prefixes: Vec<PrefixInfo>,
    },
    /// Type 135.
    NeighborSolicit {
        target: Ipv6Addr,
        mac: Option<MacAddr>,
    },
    /// Type 136.
    NeighborAdvert {
        target: Ipv6Addr,
        router: bool,
        solicited: bool,
        mac: Option<MacAddr>,
    },
}

/// Destination-unreachable codes used here.
pub const UNREACH6_NO_ROUTE: u8 = 0;
pub const UNREACH6_ADDRESS: u8 = 3;

/// A link-layer address option: type 1 (source) or 2 (target).
fn lla(kind: u8, mac: Option<MacAddr>) -> Vec<u8> {
    mac.map(|m| {
        let mut v = vec![kind, 1];
        v.extend_from_slice(&m.0);
        v
    })
    .unwrap_or_default()
}

impl Icmp6 {
    /// Encodes with the checksum over the IPv6 pseudo-header.
    pub fn encode(&self, src: Ipv6Addr, dst: Ipv6Addr) -> Vec<u8> {
        let mut out = match self {
            Icmp6::Unreachable { code, original } => [vec![1, *code, 0, 0, 0, 0, 0, 0], original.clone()].concat(),
            Icmp6::TimeExceeded { original } => [vec![3, 0, 0, 0, 0, 0, 0, 0], original.clone()].concat(),
            Icmp6::EchoRequest { id, seq, data } | Icmp6::EchoReply { id, seq, data } => {
                let ty = if matches!(self, Icmp6::EchoRequest { .. }) { 128 } else { 129 };
                let mut v = vec![ty, 0, 0, 0];
                v.extend_from_slice(&id.to_be_bytes());
                v.extend_from_slice(&seq.to_be_bytes());
                v.extend_from_slice(data);
                v
            }
            Icmp6::RouterSolicit { mac } => [vec![133, 0, 0, 0, 0, 0, 0, 0], lla(1, *mac)].concat(),
            Icmp6::RouterAdvert { hop_limit, lifetime, mac, prefixes } => {
                let mut v = vec![134, 0, 0, 0, *hop_limit, 0];
                v.extend_from_slice(&lifetime.to_be_bytes());
                v.extend_from_slice(&[0; 8]); // reachable time, retransmit timer
                v.extend(lla(1, *mac));
                for p in prefixes {
                    // Prefix information: on-link and autonomous, lifetimes 30 days / 7 days.
                    v.extend_from_slice(&[3, 4, p.len, 0xc0]);
                    v.extend_from_slice(&2_592_000u32.to_be_bytes());
                    v.extend_from_slice(&604_800u32.to_be_bytes());
                    v.extend_from_slice(&[0; 4]);
                    v.extend_from_slice(&p.prefix.octets());
                }
                v
            }
            Icmp6::NeighborSolicit { target, mac } => [vec![135, 0, 0, 0, 0, 0, 0, 0], target.octets().to_vec(), lla(1, *mac)].concat(),
            Icmp6::NeighborAdvert { target, router, solicited, mac } => {
                let flags = (if *router { 0x80 } else { 0 }) | (if *solicited { 0x40 } else { 0 }) | 0x20;
                [vec![136, 0, 0, 0, flags, 0, 0, 0], target.octets().to_vec(), lla(2, *mac)].concat()
            }
        };
        let c = pseudo_checksum(src, dst, &out);
        if let Some(s) = out.get_mut(2..4) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        out
    }

    pub fn decode(b: &[u8], src: Ipv6Addr, dst: Ipv6Addr) -> Result<Icmp6> {
        let t = || WireError::Truncated("icmpv6");
        if b.len() < 8 {
            return Err(t());
        }
        if pseudo_checksum(src, dst, b) != 0 {
            return Err(WireError::Checksum("icmpv6"));
        }
        let (ty, code) = (b[0], b[1]);
        let rest = || b.get(8..).unwrap_or_default().to_vec();
        // Options start after the fixed part; each is (type, length in 8-byte units, data).
        let options = |from: usize| -> Vec<(u8, &[u8])> {
            let mut out = Vec::new();
            let mut at = from;
            while let (Some(&kind), Some(&units)) = (b.get(at), b.get(at + 1)) {
                let len = usize::from(units) * 8;
                let Some(body) = b.get(at..at + len).filter(|_| len > 0) else { break };
                out.push((kind, body));
                at += len;
            }
            out
        };
        let mac_opt = |from: usize, kind: u8| options(from).into_iter().find(|(k, _)| *k == kind).and_then(|(_, body)| mac_at(body, 2));
        match ty {
            1 => Ok(Icmp6::Unreachable { code, original: rest() }),
            3 => Ok(Icmp6::TimeExceeded { original: rest() }),
            128 | 129 => {
                let (id, seq) = (be16(b, 4).ok_or_else(t)?, be16(b, 6).ok_or_else(t)?);
                let data = rest();
                Ok(if ty == 128 { Icmp6::EchoRequest { id, seq, data } } else { Icmp6::EchoReply { id, seq, data } })
            }
            133 => Ok(Icmp6::RouterSolicit { mac: mac_opt(8, 1) }),
            134 => {
                if b.len() < 16 {
                    return Err(t());
                }
                let prefixes = options(16)
                    .into_iter()
                    .filter(|(k, _)| *k == 3)
                    .filter_map(|(_, body)| Some(PrefixInfo { len: *body.get(2)?, prefix: ip6_at(body, 16)? }))
                    .collect();
                Ok(Icmp6::RouterAdvert { hop_limit: b[4], lifetime: be16(b, 6).ok_or_else(t)?, mac: mac_opt(16, 1), prefixes })
            }
            135 => Ok(Icmp6::NeighborSolicit { target: ip6_at(b, 8).ok_or_else(t)?, mac: mac_opt(24, 1) }),
            136 => {
                let flags = b[4];
                Ok(Icmp6::NeighborAdvert {
                    target: ip6_at(b, 8).ok_or_else(t)?,
                    router: flags & 0x80 != 0,
                    solicited: flags & 0x40 != 0,
                    mac: mac_opt(24, 2),
                })
            }
            other => Err(WireError::Unsupported("icmpv6", format!("type {other}"))),
        }
    }

    /// What an error message quotes: as much of the packet as fits a minimum-MTU message.
    pub fn quote(packet: &[u8]) -> Vec<u8> {
        packet.get(..packet.len().min(1_232)).unwrap_or_default().to_vec()
    }

    /// For an error about one of our echo requests: its (id, seq).
    pub fn quoted_echo(original: &[u8]) -> Option<(u16, u16)> {
        if *original.get(6)? != PROTO_ICMPV6 || *original.get(40)? != 128 {
            return None;
        }
        Some((be16(original, 44)?, be16(original, 46)?))
    }
}

fn pseudo_checksum(src: Ipv6Addr, dst: Ipv6Addr, body: &[u8]) -> u16 {
    let len = u32::try_from(body.len()).unwrap_or(u32::MAX).to_be_bytes();
    checksum(&[&src.octets(), &dst.octets(), &len, &[0, 0, 0, PROTO_ICMPV6], body])
}

/// The event-list line for an IPv6 packet: (whether it is neighbour discovery, text).
pub fn describe(ip: &Ipv6) -> (bool, String) {
    let route = format!("{} → {}", ip.src, ip.dst);
    if ip.next_header != PROTO_ICMPV6 {
        return (false, format!("IPv6 next header {} {route}", ip.next_header));
    }
    match Icmp6::decode(&ip.payload, ip.src, ip.dst) {
        Ok(Icmp6::EchoRequest { seq, .. }) => (false, format!("ICMPv6 echo request seq {seq} {route} hop limit {}", ip.hop_limit)),
        Ok(Icmp6::EchoReply { seq, .. }) => (false, format!("ICMPv6 echo reply seq {seq} {route} hop limit {}", ip.hop_limit)),
        Ok(Icmp6::Unreachable { code, .. }) => (false, format!("ICMPv6 destination unreachable (code {code}) {route}")),
        Ok(Icmp6::TimeExceeded { .. }) => (false, format!("ICMPv6 time exceeded {route}")),
        Ok(Icmp6::RouterSolicit { .. }) => (true, format!("NDP router solicitation from {}", ip.src)),
        Ok(Icmp6::RouterAdvert { prefixes, .. }) => {
            let p: Vec<String> = prefixes.iter().map(|p| format!("{}/{}", p.prefix, p.len)).collect();
            (true, format!("NDP router advertisement from {} (prefix {})", ip.src, if p.is_empty() { "none".into() } else { p.join(", ") }))
        }
        Ok(Icmp6::NeighborSolicit { target, .. }) => (true, format!("NDP who has {target}? tell {}", ip.src)),
        Ok(Icmp6::NeighborAdvert { target, mac, .. }) => {
            (true, format!("NDP {target} is at {}", mac.map_or("(no address given)".to_string(), |m| m.to_string())))
        }
        Err(e) => (false, format!("ICMPv6 ({e}) {route}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> Ipv6Addr {
        s.parse().unwrap()
    }

    #[test]
    fn multicast_groups() {
        assert_eq!(solicited_node(a("2001:db8::1:2:3")).to_string(), "ff02::1:ff02:3");
        assert_eq!(multicast_mac(a("ff02::1:ff02:3")).to_string(), "33:33:ff:02:00:03");
    }

    #[test]
    fn messages_round_trip_with_their_checksum() {
        let (s, d) = (a("fe80::1"), a("ff02::1"));
        let m = MacAddr([2, 0, 0, 0, 0, 1]);
        let all = [
            Icmp6::EchoRequest { id: 1, seq: 2, data: vec![9; 10] },
            Icmp6::EchoReply { id: 1, seq: 2, data: vec![] },
            Icmp6::Unreachable { code: 3, original: vec![1, 2, 3] },
            Icmp6::TimeExceeded { original: vec![] },
            Icmp6::RouterSolicit { mac: Some(m) },
            Icmp6::RouterAdvert { hop_limit: 64, lifetime: 1800, mac: Some(m), prefixes: vec![PrefixInfo { prefix: a("2001:db8:1::"), len: 64 }] },
            Icmp6::NeighborSolicit { target: a("2001:db8::5"), mac: Some(m) },
            Icmp6::NeighborAdvert { target: a("2001:db8::5"), router: true, solicited: true, mac: Some(m) },
        ];
        for msg in all {
            let b = msg.encode(s, d);
            assert_eq!(Icmp6::decode(&b, s, d).unwrap(), msg);
            assert_eq!(Icmp6::decode(&b, s, a("ff02::2")), Err(WireError::Checksum("icmpv6")));
            for n in 0..b.len() {
                let _ = Icmp6::decode(&b[..n], s, d);
            }
        }
    }

    #[test]
    fn packets_round_trip_and_quote_the_echo() {
        let (s, d) = (a("2001:db8::1"), a("2001:db8::2"));
        let ip = Ipv6 {
            hop_limit: 64,
            next_header: PROTO_ICMPV6,
            src: s,
            dst: d,
            payload: Icmp6::EchoRequest { id: 7, seq: 3, data: vec![0; 8] }.encode(s, d),
        };
        let b = ip.encode();
        assert_eq!(Ipv6::decode(&b).unwrap(), ip);
        assert_eq!(Icmp6::quoted_echo(&Icmp6::quote(&b)), Some((7, 3)));
        assert_eq!(describe(&ip).1, "ICMPv6 echo request seq 3 2001:db8::1 → 2001:db8::2 hop limit 64");
        for n in 0..b.len() {
            let _ = Ipv6::decode(&b[..n]);
        }
    }
}
