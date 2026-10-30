//! DHCPv6 (RFC 8415): the messages and options a simple server and client use.
//!
//! Messages: Solicit, Advertise, Request, Reply, Information-request. Options: client and
//! server identifiers (DUID-LL), IA_NA with one IA Address, DNS servers (RFC 3646) and the
//! domain search list. Relay agents wrap client messages in Relay-forward and unwrap the
//! server's Relay-reply (§9).

use std::net::Ipv6Addr;

use netlabcraft_core::MacAddr;

use crate::{Result, WireError, be16};

pub const CLIENT_PORT: u16 = 546;
pub const SERVER_PORT: u16 = 547;
/// All DHCP relay agents and servers (`ff02::1:2`).
pub const ALL_SERVERS: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 1, 2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Solicit = 1,
    Advertise = 2,
    Request = 3,
    Reply = 7,
    InformationRequest = 11,
}

impl Kind {
    fn from_u8(v: u8) -> Option<Kind> {
        Some(match v {
            1 => Kind::Solicit,
            2 => Kind::Advertise,
            3 => Kind::Request,
            7 => Kind::Reply,
            11 => Kind::InformationRequest,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Solicit => "Solicit",
            Kind::Advertise => "Advertise",
            Kind::Request => "Request",
            Kind::Reply => "Reply",
            Kind::InformationRequest => "Information-request",
        }
    }
}

/// One DHCPv6 message, with the options this simulator uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dhcp6 {
    pub kind: Kind,
    /// 24-bit transaction id.
    pub xid: u32,
    pub client: Option<MacAddr>,
    pub server: Option<MacAddr>,
    /// IA_NA: the identity association id and the address offered or given.
    pub ia: Option<(u32, Option<Ipv6Addr>)>,
    pub dns: Vec<Ipv6Addr>,
    pub domain: Option<String>,
}

const OPT_CLIENTID: u16 = 1;
const OPT_SERVERID: u16 = 2;
const OPT_IA_NA: u16 = 3;
const OPT_IAADDR: u16 = 5;
const OPT_DNS: u16 = 23;
const OPT_DOMAINS: u16 = 24;

const OPT_RELAY_MSG: u16 = 9;

/// A Relay-forward (relay agent to server) or Relay-reply (server to relay agent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    pub reply: bool,
    pub hops: u8,
    /// An address of the relay's interface towards the client: it picks the client's link.
    pub link: Ipv6Addr,
    /// The client's address (its link-local one).
    pub peer: Ipv6Addr,
    /// The message relayed, encoded.
    pub message: Vec<u8>,
}

impl Relay {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = vec![if self.reply { 13 } else { 12 }, self.hops];
        v.extend_from_slice(&self.link.octets());
        v.extend_from_slice(&self.peer.octets());
        v.extend(option(OPT_RELAY_MSG, &self.message));
        v
    }

    /// `None` when the message isn't a relay message at all.
    pub fn decode(b: &[u8]) -> Option<Result<Relay>> {
        let reply = match b.first()? {
            12 => false,
            13 => true,
            _ => return None,
        };
        Some(Relay::fields(b, reply))
    }

    fn fields(b: &[u8], reply: bool) -> Result<Relay> {
        let t = || WireError::Truncated("dhcpv6 relay");
        let addr = |at: usize| b.get(at..at + 16).and_then(|o| <[u8; 16]>::try_from(o).ok()).map(Ipv6Addr::from).ok_or_else(t);
        let (link, peer) = (addr(2)?, addr(18)?);
        let message =
            options(b.get(34..).unwrap_or_default()).into_iter().find(|(c, _)| *c == OPT_RELAY_MSG).map(|(_, m)| m.to_vec()).ok_or_else(t)?;
        Ok(Relay { reply, hops: *b.get(1).ok_or_else(t)?, link, peer, message })
    }
}

fn option(code: u16, body: &[u8]) -> Vec<u8> {
    let len = u16::try_from(body.len()).unwrap_or(u16::MAX);
    [&code.to_be_bytes()[..], &len.to_be_bytes(), body].concat()
}

/// DUID-LL (type 3, hardware type 1 Ethernet) of a MAC address.
fn duid(mac: MacAddr) -> Vec<u8> {
    [&[0, 3, 0, 1][..], &mac.0].concat()
}

fn duid_mac(b: &[u8]) -> Option<MacAddr> {
    if b.get(..4)? != [0, 3, 0, 1] {
        return None;
    }
    crate::mac_at(b, 4)
}

/// Options as (code, body) pairs; stops at the first malformed one.
fn options(mut b: &[u8]) -> Vec<(u16, &[u8])> {
    let mut out = Vec::new();
    while b.len() >= 4 {
        let (Some(code), Some(len)) = (be16(b, 0), be16(b, 2)) else { break };
        let Some(body) = b.get(4..4 + usize::from(len)) else { break };
        out.push((code, body));
        b = b.get(4 + usize::from(len)..).unwrap_or_default();
    }
    out
}

impl Dhcp6 {
    pub fn encode(&self) -> Vec<u8> {
        let x = self.xid.to_be_bytes();
        let mut v = vec![self.kind as u8, x[1], x[2], x[3]];
        if let Some(m) = self.client {
            v.extend(option(OPT_CLIENTID, &duid(m)));
        }
        if let Some(m) = self.server {
            v.extend(option(OPT_SERVERID, &duid(m)));
        }
        if let Some((iaid, addr)) = self.ia {
            // IAID, T1 and T2 (0: the client chooses), then the address with its lifetimes.
            let mut body = iaid.to_be_bytes().to_vec();
            body.extend_from_slice(&[0; 8]);
            if let Some(a) = addr {
                let mut ad = a.octets().to_vec();
                ad.extend_from_slice(&86_400u32.to_be_bytes());
                ad.extend_from_slice(&172_800u32.to_be_bytes());
                body.extend(option(OPT_IAADDR, &ad));
            }
            v.extend(option(OPT_IA_NA, &body));
        }
        if !self.dns.is_empty() {
            let body: Vec<u8> = self.dns.iter().flat_map(|a| a.octets()).collect();
            v.extend(option(OPT_DNS, &body));
        }
        if let Some(d) = &self.domain {
            // Domain names in DNS wire format: length-prefixed labels, then a zero.
            let mut body = Vec::new();
            for label in d.split('.').filter(|l| !l.is_empty()) {
                let l = &label.as_bytes()[..label.len().min(63)];
                body.push(l.len() as u8);
                body.extend_from_slice(l);
            }
            body.push(0);
            v.extend(option(OPT_DOMAINS, &body));
        }
        v
    }

    pub fn decode(b: &[u8]) -> Result<Dhcp6> {
        let t = || WireError::Truncated("dhcpv6");
        let kind = Kind::from_u8(*b.first().ok_or_else(t)?).ok_or_else(|| WireError::Unsupported("dhcpv6", format!("message type {}", b[0])))?;
        let xid = u32::from_be_bytes([0, *b.get(1).ok_or_else(t)?, *b.get(2).ok_or_else(t)?, *b.get(3).ok_or_else(t)?]);
        let mut m = Dhcp6 { kind, xid, client: None, server: None, ia: None, dns: Vec::new(), domain: None };
        for (code, body) in options(b.get(4..).unwrap_or_default()) {
            match code {
                OPT_CLIENTID => m.client = duid_mac(body),
                OPT_SERVERID => m.server = duid_mac(body),
                OPT_IA_NA if body.len() >= 12 => {
                    let iaid = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                    let addr = options(body.get(12..).unwrap_or_default()).into_iter().find(|(c, _)| *c == OPT_IAADDR).and_then(|(_, a)| {
                        let o: [u8; 16] = a.get(..16)?.try_into().ok()?;
                        Some(Ipv6Addr::from(o))
                    });
                    m.ia = Some((iaid, addr));
                }
                OPT_DNS => {
                    m.dns = body.as_chunks::<16>().0.iter().map(|c| Ipv6Addr::from(*c)).collect();
                }
                OPT_DOMAINS => {
                    let mut labels = Vec::new();
                    let mut at = 0;
                    while let Some(&len) = body.get(at) {
                        if len == 0 {
                            break;
                        }
                        let Some(l) = body.get(at + 1..at + 1 + usize::from(len)) else { break };
                        labels.push(String::from_utf8_lossy(l).into_owned());
                        at += 1 + usize::from(len);
                    }
                    if !labels.is_empty() {
                        m.domain = Some(labels.join("."));
                    }
                }
                _ => {}
            }
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let m = Dhcp6 {
            kind: Kind::Reply,
            xid: 0x00ab_cdef,
            client: Some(MacAddr([2, 0, 0, 0, 0, 1])),
            server: Some(MacAddr([2, 0, 0, 0, 0, 2])),
            ia: Some((7, Some("2001:db8::10".parse().unwrap()))),
            dns: vec!["2001:db8::53".parse().unwrap()],
            domain: Some("lab.local".into()),
        };
        let b = m.encode();
        assert_eq!(Dhcp6::decode(&b).unwrap(), m);
        for n in 0..b.len() {
            let _ = Dhcp6::decode(&b[..n]);
        }
        assert!(Dhcp6::decode(&[99, 0, 0, 0]).is_err());
        let r = Relay { reply: false, hops: 0, link: "2001:db8:1::1".parse().unwrap(), peer: "fe80::1".parse().unwrap(), message: b.clone() };
        let rb = r.encode();
        assert_eq!(Relay::decode(&rb).unwrap().unwrap(), r);
        assert!(Relay::decode(&b).is_none());
        for n in 0..rb.len() {
            let _ = Relay::decode(&rb[..n]);
        }
    }
}

/// UDP over IPv6 (RFC 8200 §8.1: the checksum covers the IPv6 pseudo-header and is mandatory).
pub fn udp6_encode(src: Ipv6Addr, dst: Ipv6Addr, src_port: u16, dst_port: u16, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(8 + payload.len()).unwrap_or(u16::MAX);
    let mut v = Vec::with_capacity(usize::from(len));
    v.extend_from_slice(&src_port.to_be_bytes());
    v.extend_from_slice(&dst_port.to_be_bytes());
    v.extend_from_slice(&len.to_be_bytes());
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(payload);
    let c = udp6_checksum(src, dst, &v);
    let c = if c == 0 { 0xffff } else { c };
    if let Some(s) = v.get_mut(6..8) {
        s.copy_from_slice(&c.to_be_bytes());
    }
    v
}

/// The (source port, destination port, payload) of a UDP datagram over IPv6, checksum checked.
pub fn udp6_decode(src: Ipv6Addr, dst: Ipv6Addr, b: &[u8]) -> Result<(u16, u16, Vec<u8>)> {
    let t = || WireError::Truncated("udp");
    let len = usize::from(be16(b, 4).ok_or_else(t)?);
    let d = b.get(..len).filter(|_| len >= 8).ok_or_else(t)?;
    if udp6_checksum(src, dst, d) != 0 {
        return Err(WireError::Checksum("udp"));
    }
    Ok((be16(d, 0).ok_or_else(t)?, be16(d, 2).ok_or_else(t)?, d.get(8..).unwrap_or_default().to_vec()))
}

fn udp6_checksum(src: Ipv6Addr, dst: Ipv6Addr, datagram: &[u8]) -> u16 {
    let len = u32::try_from(datagram.len()).unwrap_or(u32::MAX).to_be_bytes();
    crate::checksum(&[&src.octets(), &dst.octets(), &len, &[0, 0, 0, crate::PROTO_UDP], datagram])
}

#[cfg(test)]
mod udp_tests {
    use super::*;

    #[test]
    fn udp6_round_trips_with_its_checksum() {
        let (s, d) = ("fe80::1".parse().unwrap(), ALL_SERVERS);
        let b = udp6_encode(s, d, CLIENT_PORT, SERVER_PORT, &[1, 2, 3]);
        assert_eq!(udp6_decode(s, d, &b).unwrap(), (CLIENT_PORT, SERVER_PORT, vec![1, 2, 3]));
        assert!(udp6_decode(s, "ff02::2".parse().unwrap(), &b).is_err());
    }
}
