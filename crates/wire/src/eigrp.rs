//! EIGRP packets (RFC 7868): the fixed header (§6.4), the hello's TLVs — parameters (§6.6.1)
//! and software version (§6.6.4) — and the classic IPv4 internal route TLV that updates,
//! queries and replies carry. Hellos, updates and queries go to `224.0.0.10` as IP protocol 88;
//! replies and acknowledgements (hellos with no TLV and an acknowledgement number) are unicast.

use std::net::Ipv4Addr;

use netlabcraft_core::Ipv4Net;

use crate::{Result, WireError, be16, checksum};

/// EIGRP's IP protocol number and its routers group (`224.0.0.10`).
pub const PROTO_EIGRP: u8 = 88;
pub const EIGRP_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 10);
/// The header version this encoder writes.
pub const VERSION: u8 = 2;
pub const OPCODE_UPDATE: u8 = 1;
pub const OPCODE_QUERY: u8 = 3;
pub const OPCODE_REPLY: u8 = 4;
pub const OPCODE_HELLO: u8 = 5;
/// Hello interval and hold time on LAN and point-to-point links (seconds).
pub const HELLO_SECS: u16 = 5;
pub const HOLD_SECS: u16 = 15;
const HEADER_LEN: usize = 20;
const TLV_PARAMETERS: u16 = 0x0001;
const TLV_SOFTWARE: u16 = 0x0004;
/// Classic IPv4 internal route.
const TLV_INTERNAL: u16 = 0x0102;
/// Header flags: init (first update to a new neighbour), conditional receive, restart, end of
/// table.
pub const FLAG_INIT: u32 = 0x1;
pub const FLAG_CR: u32 = 0x2;
pub const FLAG_RS: u32 = 0x4;
pub const FLAG_EOT: u32 = 0x8;
/// The scaled delay that means "unreachable".
pub const DELAY_UNREACHABLE: u32 = u32::MAX;
/// Most TLVs read from one packet (hostile input).
const MAX_TLVS: usize = 64;

/// The name of an opcode, for the event list.
pub fn opcode_name(op: u8) -> &'static str {
    match op {
        OPCODE_UPDATE => "update",
        OPCODE_QUERY => "query",
        OPCODE_REPLY => "reply",
        OPCODE_HELLO => "hello",
        10 => "SIA query",
        11 => "SIA reply",
        _ => "packet",
    }
}

/// An EIGRP hello: header fields plus the parameter and software-version TLVs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EigrpHello {
    pub asn: u16,
    /// K1..K6: the metric weights; routers only become neighbours when theirs agree.
    pub k: [u8; 6],
    pub hold_secs: u16,
    /// Software version TLV: (OS major, OS minor, EIGRP major, EIGRP minor).
    pub software: Option<[u8; 4]>,
}

impl EigrpHello {
    /// The classic weights: bandwidth and delay only (K1 = K3 = 1).
    pub const DEFAULT_K: [u8; 6] = [1, 0, 1, 0, 0, 0];

    pub fn new(asn: u16) -> EigrpHello {
        EigrpHello { asn, k: Self::DEFAULT_K, hold_secs: HOLD_SECS, software: Some([1, 0, 2, 0]) }
    }

    /// A hello announcing a given hold time.
    pub fn with_hold(asn: u16, hold_secs: u16) -> EigrpHello {
        EigrpHello { hold_secs, ..EigrpHello::new(asn) }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(40);
        v.push(VERSION);
        v.push(OPCODE_HELLO);
        v.extend_from_slice(&[0, 0]); // checksum, filled below
        v.extend_from_slice(&0u32.to_be_bytes()); // flags
        v.extend_from_slice(&0u32.to_be_bytes()); // sequence: hellos are unreliable
        v.extend_from_slice(&0u32.to_be_bytes()); // acknowledgement
        v.extend_from_slice(&0u16.to_be_bytes()); // virtual router id: unicast
        v.extend_from_slice(&self.asn.to_be_bytes());
        v.extend_from_slice(&TLV_PARAMETERS.to_be_bytes());
        v.extend_from_slice(&12u16.to_be_bytes());
        v.extend_from_slice(&self.k);
        v.extend_from_slice(&self.hold_secs.to_be_bytes());
        if let Some(sw) = self.software {
            v.extend_from_slice(&TLV_SOFTWARE.to_be_bytes());
            v.extend_from_slice(&8u16.to_be_bytes());
            v.extend_from_slice(&sw);
        }
        let c = checksum(&[&v]);
        if let Some(s) = v.get_mut(2..4) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        v
    }

    /// The opcode and autonomous system of any EIGRP packet (header only, checksum verified).
    pub fn header(b: &[u8]) -> Result<(u8, u16)> {
        let t = || WireError::Truncated("eigrp");
        if b.len() < HEADER_LEN {
            return Err(t());
        }
        if b.first() != Some(&VERSION) {
            return Err(WireError::Unsupported("eigrp", format!("version {}", b.first().copied().unwrap_or(0))));
        }
        if checksum(&[b]) != 0 {
            return Err(WireError::Checksum("eigrp"));
        }
        Ok((*b.get(1).ok_or_else(t)?, be16(b, 18).ok_or_else(t)?))
    }

    pub fn decode(b: &[u8]) -> Result<EigrpHello> {
        let t = || WireError::Truncated("eigrp");
        let (op, asn) = Self::header(b)?;
        if op != OPCODE_HELLO {
            return Err(WireError::Unsupported("eigrp", format!("not a hello ({})", opcode_name(op))));
        }
        let mut params = None;
        let mut software = None;
        let mut at = HEADER_LEN;
        for _ in 0..MAX_TLVS {
            if at >= b.len() {
                break;
            }
            let kind = be16(b, at).ok_or_else(t)?;
            let len = usize::from(be16(b, at + 2).ok_or_else(t)?);
            if len < 4 {
                return Err(WireError::Unsupported("eigrp", format!("TLV length {len}")));
            }
            let value = b.get(at + 4..at + len).ok_or_else(t)?;
            match kind {
                TLV_PARAMETERS => {
                    let k: [u8; 6] = value.get(..6).ok_or_else(t)?.try_into().map_err(|_| t())?;
                    params = Some((k, be16(value, 6).ok_or_else(t)?));
                }
                TLV_SOFTWARE => {
                    software = Some(value.get(..4).ok_or_else(t)?.try_into().map_err(|_| t())?);
                }
                _ => {}
            }
            at += len;
        }
        let (k, hold_secs) = params.ok_or_else(|| WireError::Unsupported("eigrp", "hello without parameters".into()))?;
        Ok(EigrpHello { asn, k, hold_secs, software })
    }
}

/// An IPv4 internal route (classic metric, RFC 7868 §6.9.2): the metric fields are scaled as
/// the classic composite metric uses them (delay in 1/256 of ten microseconds, bandwidth as
/// `256 × 10⁷ / kbit/s`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InternalRoute {
    /// `0.0.0.0`: through the router that sent the packet.
    pub next_hop: Ipv4Addr,
    pub delay: u32,
    pub bandwidth: u32,
    /// Path MTU (24 bits on the wire).
    pub mtu: u32,
    pub hops: u8,
    pub reliability: u8,
    pub load: u8,
    pub tag: u8,
    pub flags: u8,
    pub net: Ipv4Net,
}

impl InternalRoute {
    /// A route with a path's slowest bandwidth (kbit/s) and total delay (tens of µs).
    pub fn new(net: Ipv4Net, bw_kbit: u32, delay_tens_us: u64, hops: u8) -> InternalRoute {
        let delay = u32::try_from(delay_tens_us.saturating_mul(256)).unwrap_or(DELAY_UNREACHABLE - 1).min(DELAY_UNREACHABLE - 1);
        let bandwidth = u32::try_from(2_560_000_000u64 / u64::from(bw_kbit.max(1))).unwrap_or(u32::MAX);
        InternalRoute {
            next_hop: Ipv4Addr::UNSPECIFIED,
            delay,
            bandwidth,
            mtu: 1500,
            hops,
            reliability: 255,
            load: 1,
            tag: 0,
            flags: 0,
            net: net.network(),
        }
    }

    /// The same destination announced as unreachable (infinite delay).
    pub fn unreachable(net: Ipv4Net) -> InternalRoute {
        InternalRoute { delay: DELAY_UNREACHABLE, bandwidth: 0, hops: 0, ..InternalRoute::new(net, 1, 0, 0) }
    }

    pub fn is_unreachable(&self) -> bool {
        self.delay == DELAY_UNREACHABLE
    }

    fn encode_into(&self, v: &mut Vec<u8>) {
        let dest_len = usize::from(self.net.prefix.div_ceil(8));
        let len = u16::try_from(25 + dest_len).unwrap_or(u16::MAX);
        v.extend_from_slice(&TLV_INTERNAL.to_be_bytes());
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(&self.next_hop.octets());
        v.extend_from_slice(&self.delay.to_be_bytes());
        v.extend_from_slice(&self.bandwidth.to_be_bytes());
        let mtu = self.mtu.min(0x00ff_ffff).to_be_bytes();
        v.extend_from_slice(mtu.get(1..).unwrap_or_default());
        v.extend_from_slice(&[self.hops, self.reliability, self.load, self.tag, self.flags, self.net.prefix]);
        v.extend_from_slice(self.net.addr.octets().get(..dest_len.min(4)).unwrap_or_default());
    }

    fn decode(value: &[u8]) -> Result<InternalRoute> {
        let t = || WireError::Truncated("eigrp");
        let ip = |at: usize| -> Result<Ipv4Addr> {
            let s: [u8; 4] = value.get(at..at + 4).ok_or_else(t)?.try_into().map_err(|_| t())?;
            Ok(Ipv4Addr::from(s))
        };
        let u32_at = |at: usize| -> Result<u32> { Ok(u32::from(ip(at)?)) };
        let byte = |k: usize| value.get(k).copied().ok_or_else(t);
        let prefix = byte(20)?;
        if prefix > 32 {
            return Err(WireError::Unsupported("eigrp", format!("prefix length {prefix}")));
        }
        let dest_len = usize::from(prefix.div_ceil(8));
        let dest = value.get(21..21 + dest_len).ok_or_else(t)?;
        let mut octets = [0u8; 4];
        for (o, b) in octets.iter_mut().zip(dest) {
            *o = *b;
        }
        let mtu = u32::from_be_bytes([0, byte(12)?, byte(13)?, byte(14)?]);
        Ok(InternalRoute {
            next_hop: ip(0)?,
            delay: u32_at(4)?,
            bandwidth: u32_at(8)?,
            mtu,
            hops: byte(15)?,
            reliability: byte(16)?,
            load: byte(17)?,
            tag: byte(18)?,
            flags: byte(19)?,
            net: Ipv4Net::new(Ipv4Addr::from(octets), prefix).network(),
        })
    }
}

/// A TLV of an EIGRP packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tlv {
    Parameters { k: [u8; 6], hold_secs: u16 },
    Software([u8; 4]),
    Internal(InternalRoute),
}

/// Any EIGRP packet: header fields and the TLVs understood (others are skipped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EigrpPacket {
    pub opcode: u8,
    pub flags: u32,
    pub seq: u32,
    pub ack: u32,
    pub asn: u16,
    pub tlvs: Vec<Tlv>,
}

impl EigrpPacket {
    /// An acknowledgement: a hello with no TLV that acknowledges `seq`.
    pub fn ack(asn: u16, seq: u32) -> EigrpPacket {
        EigrpPacket { opcode: OPCODE_HELLO, flags: 0, seq: 0, ack: seq, asn, tlvs: Vec::new() }
    }

    /// The routes the packet carries.
    pub fn routes(&self) -> impl Iterator<Item = &InternalRoute> {
        self.tlvs.iter().filter_map(|t| match t {
            Tlv::Internal(r) => Some(r),
            _ => None,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(HEADER_LEN + 32 * self.tlvs.len());
        v.push(VERSION);
        v.push(self.opcode);
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(&self.flags.to_be_bytes());
        v.extend_from_slice(&self.seq.to_be_bytes());
        v.extend_from_slice(&self.ack.to_be_bytes());
        v.extend_from_slice(&0u16.to_be_bytes());
        v.extend_from_slice(&self.asn.to_be_bytes());
        for tlv in &self.tlvs {
            match tlv {
                Tlv::Parameters { k, hold_secs } => {
                    v.extend_from_slice(&TLV_PARAMETERS.to_be_bytes());
                    v.extend_from_slice(&12u16.to_be_bytes());
                    v.extend_from_slice(k);
                    v.extend_from_slice(&hold_secs.to_be_bytes());
                }
                Tlv::Software(sw) => {
                    v.extend_from_slice(&TLV_SOFTWARE.to_be_bytes());
                    v.extend_from_slice(&8u16.to_be_bytes());
                    v.extend_from_slice(sw);
                }
                Tlv::Internal(r) => r.encode_into(&mut v),
            }
        }
        let c = checksum(&[&v]);
        if let Some(s) = v.get_mut(2..4) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        v
    }

    pub fn decode(b: &[u8]) -> Result<EigrpPacket> {
        let t = || WireError::Truncated("eigrp");
        let (opcode, asn) = EigrpHello::header(b)?;
        let word = |at: usize| -> Result<u32> {
            let s: [u8; 4] = b.get(at..at + 4).ok_or_else(t)?.try_into().map_err(|_| t())?;
            Ok(u32::from_be_bytes(s))
        };
        let mut p = EigrpPacket { opcode, flags: word(4)?, seq: word(8)?, ack: word(12)?, asn, tlvs: Vec::new() };
        let mut at = HEADER_LEN;
        for _ in 0..MAX_TLVS {
            if at >= b.len() {
                break;
            }
            let kind = be16(b, at).ok_or_else(t)?;
            let len = usize::from(be16(b, at + 2).ok_or_else(t)?);
            if len < 4 {
                return Err(WireError::Unsupported("eigrp", format!("TLV length {len}")));
            }
            let value = b.get(at + 4..at + len).ok_or_else(t)?;
            match kind {
                TLV_PARAMETERS => {
                    let k: [u8; 6] = value.get(..6).ok_or_else(t)?.try_into().map_err(|_| t())?;
                    p.tlvs.push(Tlv::Parameters { k, hold_secs: be16(value, 6).ok_or_else(t)? });
                }
                TLV_SOFTWARE => p.tlvs.push(Tlv::Software(value.get(..4).ok_or_else(t)?.try_into().map_err(|_| t())?)),
                TLV_INTERNAL => p.tlvs.push(Tlv::Internal(InternalRoute::decode(value)?)),
                _ => {}
            }
            at += len;
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips_and_survives_truncation() {
        let h = EigrpHello::new(100);
        let v = h.encode();
        assert_eq!(v.len(), 20 + 12 + 8);
        assert_eq!(checksum(&[&v]), 0);
        assert_eq!(EigrpHello::decode(&v).unwrap(), h);
        assert_eq!(EigrpHello::header(&v).unwrap(), (OPCODE_HELLO, 100));
        for n in 0..v.len() {
            assert!(EigrpHello::decode(&v[..n]).is_err(), "{n} bytes");
        }
        let mut bad = v.clone();
        bad[21] ^= 1;
        assert_eq!(EigrpHello::decode(&bad), Err(WireError::Checksum("eigrp")));
        // A hostile TLV length is refused, never trusted.
        let mut odd = EigrpHello { software: None, ..h }.encode();
        odd[23] = 2;
        odd[2] = 0;
        odd[3] = 0;
        let c = checksum(&[&odd]);
        odd[2..4].copy_from_slice(&c.to_be_bytes());
        assert!(EigrpHello::decode(&odd).is_err());
    }

    #[test]
    fn updates_queries_and_acks_round_trip_and_survive_truncation() {
        let r = InternalRoute::new(Ipv4Net::new(Ipv4Addr::new(192, 168, 3, 0), 24), 1_544, 2_010, 1);
        // The classic scaling: 256 × 10⁷ / 1544 and the delay × 256.
        assert_eq!(r.bandwidth, 1_658_031);
        assert_eq!(r.delay, 2_010 * 256);
        let odd = InternalRoute::new(Ipv4Net::new(Ipv4Addr::new(10, 1, 128, 0), 17), 100_000, 10, 2);
        let down = InternalRoute::unreachable(Ipv4Net::new(Ipv4Addr::new(10, 0, 0, 0), 8));
        assert!(down.is_unreachable());
        let p = EigrpPacket {
            opcode: OPCODE_UPDATE,
            flags: FLAG_INIT | FLAG_EOT,
            seq: 7,
            ack: 0,
            asn: 10,
            tlvs: vec![Tlv::Internal(r), Tlv::Internal(odd), Tlv::Internal(down)],
        };
        let v = p.encode();
        assert_eq!(v.len(), 20 + (25 + 3) + (25 + 3) + (25 + 1));
        assert_eq!(checksum(&[&v]), 0);
        assert_eq!(EigrpPacket::decode(&v).unwrap(), p);
        assert_eq!(p.routes().count(), 3);
        // Every truncation fails (the checksum covers the whole packet).
        for n in 0..v.len() {
            assert!(EigrpPacket::decode(&v[..n]).is_err(), "{n} bytes");
        }
        let a = EigrpPacket::ack(10, 7);
        let v = a.encode();
        assert_eq!(v.len(), 20);
        assert_eq!(EigrpPacket::decode(&v).unwrap(), a);
        // A hello decodes as a packet too.
        let h = EigrpHello::with_hold(10, 30).encode();
        let hp = EigrpPacket::decode(&h).unwrap();
        assert!(hp.tlvs.contains(&Tlv::Parameters { k: EigrpHello::DEFAULT_K, hold_secs: 30 }));
        // A hostile prefix length is refused.
        let mut bad = EigrpPacket { tlvs: vec![Tlv::Internal(r)], ..p.clone() }.encode();
        bad[20 + 24] = 40;
        bad[2] = 0;
        bad[3] = 0;
        let c = checksum(&[&bad]);
        bad[2..4].copy_from_slice(&c.to_be_bytes());
        assert!(EigrpPacket::decode(&bad).is_err());
    }
}
