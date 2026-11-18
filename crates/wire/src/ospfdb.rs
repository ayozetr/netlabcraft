//! OSPFv2 database exchange (RFC 2328 A.3.3–A.3.6): when two routers become neighbours they
//! describe their link-state databases (DBD), request what they lack (LSR), send it (LSU) and
//! acknowledge it (LSAck). Router-LSAs (A.4.2) and the other LSA types travel as [`RawLsa`]s: a
//! header and a body the simulator builds from its database.

use std::net::Ipv4Addr;

use crate::{Result, WireError, be16, checksum, ip_at};

/// The 20-byte header every LSA starts with (A.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LsaHeader {
    pub age: u16,
    /// 1: router-LSA.
    pub kind: u8,
    pub id: Ipv4Addr,
    pub advertising: Ipv4Addr,
    pub seq: u32,
    pub checksum: u16,
    pub length: u16,
}

impl LsaHeader {
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.age.to_be_bytes());
        out.extend_from_slice(&[0x02, self.kind]);
        out.extend_from_slice(&self.id.octets());
        out.extend_from_slice(&self.advertising.octets());
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&self.checksum.to_be_bytes());
        out.extend_from_slice(&self.length.to_be_bytes());
    }

    pub fn decode(b: &[u8], at: usize) -> Option<LsaHeader> {
        Some(LsaHeader {
            age: be16(b, at)?,
            kind: *b.get(at + 3)?,
            id: ip_at(b, at + 4)?,
            advertising: ip_at(b, at + 8)?,
            seq: u32::from_be_bytes(b.get(at + 12..at + 16)?.try_into().ok()?),
            checksum: be16(b, at + 16)?,
            length: be16(b, at + 18)?,
        })
    }
}

/// One link of a router-LSA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouterLink {
    /// 1: point-to-point to a router (id = its router ID, data = our address);
    /// 3: stub network (id = network, data = mask).
    pub kind: u8,
    pub id: Ipv4Addr,
    pub data: Ipv4Addr,
    pub metric: u16,
}

/// A router-LSA: a router's links in one area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterLsa {
    pub router_id: Ipv4Addr,
    pub seq: u32,
    pub links: Vec<RouterLink>,
}

/// The ISO 8473 Fletcher checksum of an LSA (RFC 2328 §12.1.7), over everything but the age,
/// with the two check bytes placed so the whole sums to zero.
fn fletcher(lsa: &mut [u8]) {
    let Some(data) = lsa.get_mut(2..) else { return };
    // The checksum field sits at offset 16 of the LSA, 14 of the data summed.
    const AT: usize = 14;
    if data.len() < AT + 2 {
        return;
    }
    data[AT] = 0;
    data[AT + 1] = 0;
    let (mut c0, mut c1) = (0i64, 0i64);
    for b in data.iter() {
        c0 = (c0 + i64::from(*b)) % 255;
        c1 = (c1 + c0) % 255;
    }
    let after = i64::try_from(data.len() - AT - 1).unwrap_or(0);
    let mut x = (after * c0 - c1) % 255;
    if x <= 0 {
        x += 255;
    }
    let mut y = 510 - c0 - x;
    if y > 255 {
        y -= 255;
    }
    data[AT] = u8::try_from(x).unwrap_or(0);
    data[AT + 1] = u8::try_from(y).unwrap_or(0);
}

impl RouterLsa {
    pub fn encode(&self) -> Vec<u8> {
        let length = u16::try_from(24 + 12 * self.links.len()).unwrap_or(u16::MAX);
        let mut out = Vec::new();
        LsaHeader { age: 1, kind: 1, id: self.router_id, advertising: self.router_id, seq: self.seq, checksum: 0, length }.encode(&mut out);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&u16::try_from(self.links.len()).unwrap_or(u16::MAX).to_be_bytes());
        for l in &self.links {
            out.extend_from_slice(&l.id.octets());
            out.extend_from_slice(&l.data.octets());
            out.extend_from_slice(&[l.kind, 0]);
            out.extend_from_slice(&l.metric.to_be_bytes());
        }
        fletcher(&mut out);
        out
    }

    /// The LSA's header, as a DBD or an acknowledgement lists it.
    pub fn header(&self) -> LsaHeader {
        let b = self.encode();
        LsaHeader::decode(&b, 0).unwrap_or(LsaHeader {
            age: 1,
            kind: 1,
            id: self.router_id,
            advertising: self.router_id,
            seq: self.seq,
            checksum: 0,
            length: 0,
        })
    }
}

/// Whether sequence number `a` is newer than `b`: sequence numbers are signed, starting at
/// 0x80000001 (RFC 2328 §12.1.6).
pub fn seq_newer(a: u32, b: u32) -> bool {
    i32::from_be_bytes(a.to_be_bytes()) > i32::from_be_bytes(b.to_be_bytes())
}

/// The age at which an LSA is flushed (MaxAge, RFC 2328 Appendix B).
pub const MAX_AGE: u16 = 3600;

/// An LSA of any type: its header and its body, as it travels in an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLsa {
    pub header: LsaHeader,
    pub body: Vec<u8>,
}

impl RawLsa {
    /// Builds an LSA (length and checksum are worked out here).
    pub fn new(age: u16, kind: u8, id: Ipv4Addr, advertising: Ipv4Addr, seq: u32, body: Vec<u8>) -> RawLsa {
        let length = u16::try_from(20 + body.len()).unwrap_or(u16::MAX);
        let mut l = RawLsa { header: LsaHeader { age, kind, id, advertising, seq, checksum: 0, length }, body };
        let mut b = Vec::new();
        l.header.encode(&mut b);
        b.extend_from_slice(&l.body);
        fletcher(&mut b);
        l.header.checksum = be16(&b, 16).unwrap_or(0);
        l
    }

    /// The same LSA with another age (the checksum does not cover the age).
    pub fn with_age(&self, age: u16) -> RawLsa {
        let mut l = self.clone();
        l.header.age = age;
        l
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        self.header.encode(&mut b);
        b.extend_from_slice(&self.body);
        b
    }

    /// One LSA at `at`, as long as its header says (none when it does not fit).
    pub fn decode_at(b: &[u8], at: usize) -> Option<RawLsa> {
        let header = LsaHeader::decode(b, at)?;
        let len = usize::from(header.length);
        if len < 20 {
            return None;
        }
        let body = b.get(at + 20..at.checked_add(len)?)?.to_vec();
        Some(RawLsa { header, body })
    }

    /// Whether the Fletcher checksum is the one the LSA's bytes sum to.
    pub fn checksum_ok(&self) -> bool {
        let b = self.encode();
        let (mut c0, mut c1) = (0u32, 0u32);
        for x in b.iter().skip(2) {
            c0 = (c0 + u32::from(*x)) % 255;
            c1 = (c1 + c0) % 255;
        }
        (c0, c1) == (0, 0)
    }
}

/// The body of a router-LSA (A.4.2): flag bits (B 0x01, E 0x02, Nt 0x10) and the links.
pub fn router_body(flags: u8, links: &[RouterLink]) -> Vec<u8> {
    let mut out = vec![flags, 0];
    out.extend_from_slice(&u16::try_from(links.len()).unwrap_or(u16::MAX).to_be_bytes());
    for l in links {
        out.extend_from_slice(&l.id.octets());
        out.extend_from_slice(&l.data.octets());
        out.extend_from_slice(&[l.kind, 0]);
        out.extend_from_slice(&l.metric.to_be_bytes());
    }
    out
}

/// The body of a network-LSA (A.4.3): the mask and the attached routers.
pub fn network_body(mask: Ipv4Addr, attached: &[Ipv4Addr]) -> Vec<u8> {
    let mut out = mask.octets().to_vec();
    for a in attached {
        out.extend_from_slice(&a.octets());
    }
    out
}

/// The body of a summary-LSA (types 3 and 4, A.4.4): the mask and a 24-bit metric.
pub fn summary_body(mask: Ipv4Addr, metric: u32) -> Vec<u8> {
    let mut out = mask.octets().to_vec();
    out.extend_from_slice(&metric.min(0x00ff_ffff).to_be_bytes());
    out
}

/// The body of an AS-external or NSSA LSA (types 5 and 7, A.4.5): the mask, the E bit (a type 2
/// metric), a 24-bit metric, the forwarding address and the tag.
pub fn external_body(mask: Ipv4Addr, type2: bool, metric: u32, forward: Ipv4Addr, tag: u32) -> Vec<u8> {
    let mut out = mask.octets().to_vec();
    let m = metric.min(0x00ff_ffff).to_be_bytes();
    out.extend_from_slice(&[if type2 { 0x80 } else { 0 }, m[1], m[2], m[3]]);
    out.extend_from_slice(&forward.octets());
    out.extend_from_slice(&tag.to_be_bytes());
    out
}

/// DBD flags.
pub const DBD_INIT: u8 = 4;
pub const DBD_MORE: u8 = 2;
pub const DBD_MASTER: u8 = 1;

/// A database-exchange packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OspfDb {
    Dbd {
        flags: u8,
        seq: u32,
        headers: Vec<LsaHeader>,
    },
    /// (LS type, link-state ID, advertising router).
    Request(Vec<(u32, Ipv4Addr, Ipv4Addr)>),
    Update(Vec<RawLsa>),
    Ack(Vec<LsaHeader>),
}

/// A whole OSPF packet: the common header (A.3.1) and its body.
pub fn packet(kind: u8, router_id: Ipv4Addr, area: u32, body: &[u8]) -> Vec<u8> {
    let len = u16::try_from(24 + body.len()).unwrap_or(u16::MAX);
    let mut v = vec![2, kind];
    v.extend_from_slice(&len.to_be_bytes());
    v.extend_from_slice(&router_id.octets());
    v.extend_from_slice(&area.to_be_bytes());
    v.extend_from_slice(&[0; 12]);
    v.extend_from_slice(body);
    let c = checksum(&[v.get(..16).unwrap_or_default(), v.get(24..).unwrap_or_default()]);
    if let Some(s) = v.get_mut(12..14) {
        s.copy_from_slice(&c.to_be_bytes());
    }
    v
}

impl OspfDb {
    pub fn kind(&self) -> u8 {
        match self {
            OspfDb::Dbd { .. } => 2,
            OspfDb::Request(_) => 3,
            OspfDb::Update(_) => 4,
            OspfDb::Ack(_) => 5,
        }
    }

    pub fn encode(&self, router_id: Ipv4Addr, area: u32) -> Vec<u8> {
        let mut body = Vec::new();
        match self {
            OspfDb::Dbd { flags, seq, headers } => {
                body.extend_from_slice(&1500u16.to_be_bytes());
                body.extend_from_slice(&[0x02, *flags]);
                body.extend_from_slice(&seq.to_be_bytes());
                for h in headers {
                    h.encode(&mut body);
                }
            }
            OspfDb::Request(list) => {
                for (t, id, adv) in list {
                    body.extend_from_slice(&t.to_be_bytes());
                    body.extend_from_slice(&id.octets());
                    body.extend_from_slice(&adv.octets());
                }
            }
            OspfDb::Update(lsas) => {
                body.extend_from_slice(&u32::try_from(lsas.len()).unwrap_or(0).to_be_bytes());
                for l in lsas {
                    body.extend(l.encode());
                }
            }
            OspfDb::Ack(headers) => {
                for h in headers {
                    h.encode(&mut body);
                }
            }
        }
        packet(self.kind(), router_id, area, &body)
    }

    /// The packet and the router that sent it.
    pub fn decode(b: &[u8]) -> Result<(Ipv4Addr, OspfDb)> {
        let t = || WireError::Truncated("ospf");
        if b.first() != Some(&2) {
            return Err(WireError::Unsupported("ospf", "not OSPFv2".into()));
        }
        let len = usize::from(be16(b, 2).ok_or_else(t)?);
        let p = b.get(..len).filter(|_| len >= 24).ok_or_else(t)?;
        if be16(p, 14) != Some(2) && checksum(&[p.get(..16).unwrap_or_default(), p.get(24..).unwrap_or_default()]) != 0 {
            return Err(WireError::Checksum("ospf"));
        }
        let router = ip_at(p, 4).ok_or_else(t)?;
        let headers = |from: usize| (from..len).step_by(20).map_while(|at| LsaHeader::decode(p, at)).collect::<Vec<_>>();
        let db = match p.get(1) {
            Some(2) => OspfDb::Dbd {
                flags: *p.get(27).ok_or_else(t)?,
                seq: u32::from_be_bytes(p.get(28..32).ok_or_else(t)?.try_into().map_err(|_| t())?),
                headers: headers(32),
            },
            Some(3) => OspfDb::Request(
                (24..len)
                    .step_by(12)
                    .map_while(|at| Some((u32::from_be_bytes(p.get(at..at + 4)?.try_into().ok()?), ip_at(p, at + 4)?, ip_at(p, at + 8)?)))
                    .collect(),
            ),
            Some(4) => {
                let count = u32::from_be_bytes(p.get(24..28).ok_or_else(t)?.try_into().map_err(|_| t())?);
                let mut lsas = Vec::new();
                let mut at = 28;
                for _ in 0..count.min(256) {
                    let Some(l) = RawLsa::decode_at(p, at) else { break };
                    at += usize::from(l.header.length.max(20));
                    lsas.push(l);
                }
                OspfDb::Update(lsas)
            }
            Some(5) => OspfDb::Ack(headers(24)),
            _ => return Err(WireError::Unsupported("ospf", "not a database-exchange packet".into())),
        };
        Ok((router, db))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_exchange_round_trips() {
        let a = Ipv4Addr::new(1, 1, 1, 1);
        let lsa = RouterLsa {
            router_id: a,
            seq: 0x8000_0001,
            links: vec![
                RouterLink { kind: 1, id: Ipv4Addr::new(2, 2, 2, 2), data: Ipv4Addr::new(10, 0, 0, 1), metric: 1 },
                RouterLink { kind: 3, id: Ipv4Addr::new(10, 0, 0, 0), data: Ipv4Addr::new(255, 255, 255, 252), metric: 1 },
            ],
        };
        // The Fletcher checksum verifies: summing the LSA (without its age) gives zero.
        let bytes = lsa.encode();
        let (mut c0, mut c1) = (0u32, 0u32);
        for b in &bytes[2..] {
            c0 = (c0 + u32::from(*b)) % 255;
            c1 = (c1 + c0) % 255;
        }
        assert_eq!((c0, c1), (0, 0));
        assert_ne!(lsa.header().checksum, 0);
        for p in [
            OspfDb::Dbd { flags: DBD_INIT | DBD_MORE | DBD_MASTER, seq: 7, headers: vec![] },
            OspfDb::Dbd { flags: DBD_MASTER, seq: 8, headers: vec![lsa.header()] },
            OspfDb::Request(vec![(1, a, a)]),
            OspfDb::Update(vec![RawLsa::new(5, 1, a, a, lsa.seq, router_body(0, &lsa.links))]),
            OspfDb::Ack(vec![lsa.header()]),
        ] {
            let b = p.encode(a, 0);
            assert_eq!(OspfDb::decode(&b).unwrap(), (a, p.clone()));
            for k in 0..b.len() {
                let _ = OspfDb::decode(&b[..k]);
            }
        }
    }

    #[test]
    fn every_lsa_type_round_trips_with_a_valid_checksum() {
        let (a, b) = (Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(10, 0, 0, 0));
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        let lsas = vec![
            RawLsa::new(0, 1, a, a, 0x8000_0001, router_body(0x01, &[RouterLink { kind: 3, id: b, data: mask, metric: 1 }])),
            RawLsa::new(10, 2, Ipv4Addr::new(10, 0, 0, 1), a, 0x8000_0002, network_body(mask, &[a, Ipv4Addr::new(2, 2, 2, 2)])),
            RawLsa::new(20, 3, b, a, 0x8000_0003, summary_body(mask, 20)),
            RawLsa::new(30, 4, Ipv4Addr::new(3, 3, 3, 3), a, 0x8000_0004, summary_body(Ipv4Addr::UNSPECIFIED, 5)),
            RawLsa::new(40, 5, b, a, 0x8000_0005, external_body(mask, true, 20, Ipv4Addr::UNSPECIFIED, 7)),
            RawLsa::new(50, 7, b, a, 0x8000_0006, external_body(mask, false, 20, Ipv4Addr::new(10, 0, 0, 1), 0)),
        ];
        for l in &lsas {
            assert!(l.checksum_ok(), "type {}", l.header.kind);
            assert_eq!(l.with_age(3600).header.checksum, l.header.checksum, "the age is outside the checksum");
            assert_eq!(usize::from(l.header.length), l.encode().len());
        }
        let p = OspfDb::Update(lsas.clone());
        let bytes = p.encode(a, 1);
        assert_eq!(OspfDb::decode(&bytes).unwrap(), (a, p));
        for k in 0..bytes.len() {
            let _ = OspfDb::decode(&bytes[..k]);
        }
    }

    #[test]
    fn sequence_numbers_compare_as_signed() {
        assert!(seq_newer(0x8000_0002, 0x8000_0001));
        assert!(!seq_newer(0x8000_0001, 0x8000_0001));
        assert!(seq_newer(0, 0xffff_ffff), "the number after the largest wraps to zero");
        assert!(!seq_newer(0x8000_0001, 0x7fff_ffff));
    }
}
