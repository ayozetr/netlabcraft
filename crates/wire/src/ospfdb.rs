//! OSPFv2 database exchange (RFC 2328 A.3.3–A.3.6): when two routers become neighbours they
//! describe their link-state databases (DBD), request what they lack (LSR), send it (LSU) and
//! acknowledge it (LSAck). Only router-LSAs (A.4.2) are carried here.

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
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.age.to_be_bytes());
        out.extend_from_slice(&[0x02, self.kind]);
        out.extend_from_slice(&self.id.octets());
        out.extend_from_slice(&self.advertising.octets());
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&self.checksum.to_be_bytes());
        out.extend_from_slice(&self.length.to_be_bytes());
    }

    fn decode(b: &[u8], at: usize) -> Option<LsaHeader> {
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
    Update(Vec<RouterLsa>),
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
        if checksum(&[p.get(..16).unwrap_or_default(), p.get(24..).unwrap_or_default()]) != 0 {
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
                    let Some(h) = LsaHeader::decode(p, at) else { break };
                    let n = usize::from(be16(p, at + 22).unwrap_or(0));
                    let links = (0..n.min(256))
                        .map_while(|i| {
                            let l = at + 24 + 12 * i;
                            Some(RouterLink { id: ip_at(p, l)?, data: ip_at(p, l + 4)?, kind: *p.get(l + 8)?, metric: be16(p, l + 10)? })
                        })
                        .collect();
                    lsas.push(RouterLsa { router_id: h.advertising, seq: h.seq, links });
                    at += usize::from(h.length.max(20));
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
            OspfDb::Update(vec![lsa.clone()]),
            OspfDb::Ack(vec![lsa.header()]),
        ] {
            let b = p.encode(a, 0);
            assert_eq!(OspfDb::decode(&b).unwrap(), (a, p.clone()));
            for k in 0..b.len() {
                let _ = OspfDb::decode(&b[..k]);
            }
        }
    }
}
