//! TCP segments (RFC 793): the header fields a lab needs, with the checksum over the IPv4
//! pseudo-header, or the IPv6 one (RFC 8200 §8.1) for [`Tcp::encode6`] and [`Tcp::decode6`].
//! Options are skipped on decode and never written.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::{PROTO_TCP, Result, WireError, checksum, pseudo_header};

pub const FIN: u8 = 0x01;
pub const SYN: u8 = 0x02;
pub const RST: u8 = 0x04;
pub const PSH: u8 = 0x08;
pub const ACK: u8 = 0x10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tcp {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    pub payload: Vec<u8>,
}

impl Tcp {
    fn header_and_payload(&self) -> Vec<u8> {
        let mut b = vec![0u8; 20];
        b[0..2].copy_from_slice(&self.src_port.to_be_bytes());
        b[2..4].copy_from_slice(&self.dst_port.to_be_bytes());
        b[4..8].copy_from_slice(&self.seq.to_be_bytes());
        b[8..12].copy_from_slice(&self.ack.to_be_bytes());
        b[12] = 5 << 4;
        b[13] = self.flags;
        b[14..16].copy_from_slice(&self.window.to_be_bytes());
        b.extend_from_slice(&self.payload);
        b
    }

    fn with_checksum(mut b: Vec<u8>, pseudo: &[u8]) -> Vec<u8> {
        let c = checksum(&[pseudo, &b]);
        if let Some(s) = b.get_mut(16..18) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        b
    }

    pub fn encode(&self, src: Ipv4Addr, dst: Ipv4Addr) -> Vec<u8> {
        let b = self.header_and_payload();
        let len = u16::try_from(b.len()).unwrap_or(u16::MAX);
        Tcp::with_checksum(b, &pseudo_header(src, dst, PROTO_TCP, len))
    }

    /// Encodes over IPv6.
    pub fn encode6(&self, src: Ipv6Addr, dst: Ipv6Addr) -> Vec<u8> {
        let b = self.header_and_payload();
        let p = pseudo6(src, dst, b.len());
        Tcp::with_checksum(b, &p)
    }

    /// Encodes over either IP version (mixed versions encode as IPv4-mapped IPv6).
    pub fn encode_ip(&self, src: IpAddr, dst: IpAddr) -> Vec<u8> {
        match (src, dst) {
            (IpAddr::V4(s), IpAddr::V4(d)) => self.encode(s, d),
            (s, d) => self.encode6(v6(s), v6(d)),
        }
    }

    pub fn decode(b: &[u8], src: Ipv4Addr, dst: Ipv4Addr) -> Result<Tcp> {
        let len = u16::try_from(b.len()).map_err(|_| WireError::Truncated("tcp"))?;
        Tcp::decode_with(b, &pseudo_header(src, dst, PROTO_TCP, len))
    }

    /// Decodes a segment that came over IPv6.
    pub fn decode6(b: &[u8], src: Ipv6Addr, dst: Ipv6Addr) -> Result<Tcp> {
        Tcp::decode_with(b, &pseudo6(src, dst, b.len()))
    }

    fn decode_with(b: &[u8], pseudo: &[u8]) -> Result<Tcp> {
        let t = || WireError::Truncated("tcp");
        if b.len() < 20 || b.len() > usize::from(u16::MAX) {
            return Err(t());
        }
        let off = usize::from(b[12] >> 4) * 4;
        if off < 20 || off > b.len() {
            return Err(t());
        }
        if checksum(&[pseudo, b]) != 0 {
            return Err(WireError::Checksum("tcp"));
        }
        let u16_at = |i: usize| u16::from_be_bytes([b[i], b[i + 1]]);
        let u32_at = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Ok(Tcp {
            src_port: u16_at(0),
            dst_port: u16_at(2),
            seq: u32_at(4),
            ack: u32_at(8),
            flags: b[13],
            window: u16_at(14),
            payload: b.get(off..).unwrap_or_default().to_vec(),
        })
    }

    /// `SYN, ACK`-style flag names.
    pub fn flag_names(&self) -> String {
        let names: Vec<&str> = [(SYN, "SYN"), (ACK, "ACK"), (PSH, "PSH"), (FIN, "FIN"), (RST, "RST")]
            .iter()
            .filter(|(f, _)| self.flags & f != 0)
            .map(|(_, n)| *n)
            .collect();
        names.join(", ")
    }
}

fn v6(a: IpAddr) -> Ipv6Addr {
    match a {
        IpAddr::V4(x) => x.to_ipv6_mapped(),
        IpAddr::V6(x) => x,
    }
}

/// The IPv6 pseudo-header (RFC 8200 §8.1): addresses, upper-layer length, next header.
fn pseudo6(src: Ipv6Addr, dst: Ipv6Addr, len: usize) -> Vec<u8> {
    let mut p = Vec::with_capacity(40);
    p.extend_from_slice(&src.octets());
    p.extend_from_slice(&dst.octets());
    p.extend_from_slice(&u32::try_from(len).unwrap_or(u32::MAX).to_be_bytes());
    p.extend_from_slice(&[0, 0, 0, PROTO_TCP]);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_checksum_and_truncation() {
        let (s, d) = (Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2));
        let seg = Tcp { src_port: 49152, dst_port: 80, seq: 1000, ack: 5, flags: SYN | ACK, window: 65535, payload: b"GET / HTTP/1.1".to_vec() };
        let b = seg.encode(s, d);
        assert_eq!(Tcp::decode(&b, s, d).unwrap(), seg);
        assert_eq!(seg.flag_names(), "SYN, ACK");
        assert_eq!(Tcp::decode(&b, s, Ipv4Addr::new(10, 0, 0, 3)), Err(WireError::Checksum("tcp")));
        for n in 0..b.len() {
            let _ = Tcp::decode(&b[..n], s, d);
        }
    }

    #[test]
    fn over_ipv6_the_checksum_covers_the_ipv6_pseudo_header() {
        let (s, d): (Ipv6Addr, Ipv6Addr) = ("2001:db8::1".parse().unwrap(), "2001:db8::2".parse().unwrap());
        let seg = Tcp { src_port: 49152, dst_port: 443, seq: 7, ack: 9, flags: PSH | ACK, window: 1000, payload: vec![0x16, 3, 3, 0, 1, 1] };
        let b = seg.encode6(s, d);
        assert_eq!(Tcp::decode6(&b, s, d).unwrap(), seg);
        assert_eq!(seg.encode_ip(IpAddr::V6(s), IpAddr::V6(d)), b);
        assert_eq!(Tcp::decode6(&b, s, "2001:db8::3".parse().unwrap()), Err(WireError::Checksum("tcp")));
        assert!(Tcp::decode(&b, Ipv4Addr::LOCALHOST, Ipv4Addr::LOCALHOST).is_err());
        for n in 0..b.len() {
            assert!(Tcp::decode6(&b[..n], s, d).is_err(), "cut {n}");
        }
    }
}
