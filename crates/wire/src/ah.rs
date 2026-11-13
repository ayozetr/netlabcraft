//! The IP Authentication Header (RFC 4302), protocol 51, over IPv4: the header (§2: next header,
//! payload length, SPI, sequence number, integrity check value), the integrity check value
//! computed as §3.3.3 has it (the IPv4 header with its mutable fields zeroed, then the AH
//! header with a zero ICV, then everything it protects), in tunnel mode (a new outer header
//! around the whole inner packet) and transport mode (AH inserted after the original header).
//!
//! The integrity algorithms are the ones IPsec names for AH: HMAC-MD5-96 (RFC 2403),
//! HMAC-SHA1-96 (RFC 2404) and HMAC-SHA-256-128 (RFC 4868), see [`crate::suite::Integ`].
//!
//! Mutable IPv4 fields (§3.3.3.1.1.1) are zeroed for the ICV: DS field and ECN, the flags
//! (DF, MF), the fragment offset, the TTL and the header checksum. Version, header length, total
//! length, identification, protocol (51), source and destination are covered. This simulator's
//! IPv4 header has no options, so the 20-byte header is all there is.

use crate::suite::Integ;
use crate::{Ipv4, Result, WireError};

/// IP protocol number of AH.
pub const PROTO_AH: u8 = 51;
/// Bytes of the fixed part of the header, before the ICV.
const FIXED: usize = 12;
/// Longest ICV accepted (hostile input): well beyond the 128 bits any suite here has.
const MAX_ICV: usize = 64;

/// An AH header and what follows it (the protected payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ah {
    /// The protocol of what follows (4 for an IPv4 packet in tunnel mode).
    pub next: u8,
    pub spi: u32,
    pub seq: u32,
    pub icv: Vec<u8>,
    pub payload: Vec<u8>,
}

impl Ah {
    /// The header (with `icv` as given) and the payload. The ICV is a multiple of 32 bits for
    /// every suite here, so no padding is needed (§2.6).
    pub fn encode(&self) -> Vec<u8> {
        let words = (FIXED + self.icv.len()) / 4;
        let mut out = Vec::with_capacity(FIXED + self.icv.len() + self.payload.len());
        // Payload Len: the header's length in 32-bit words, minus 2 (§2.2).
        out.extend_from_slice(&[self.next, u8::try_from(words.saturating_sub(2)).unwrap_or(0), 0, 0]);
        out.extend_from_slice(&self.spi.to_be_bytes());
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&self.icv);
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Ah> {
        let t = || WireError::Truncated("ah");
        let next = *b.first().ok_or_else(t)?;
        let words = usize::from(*b.get(1).ok_or_else(t)?) + 2;
        let head = words * 4;
        let icv_len = head.checked_sub(FIXED).ok_or_else(t)?;
        if icv_len > MAX_ICV {
            return Err(WireError::Unsupported("ah", format!("ICV of {icv_len} bytes")));
        }
        let word = |at: usize| -> Result<u32> { Ok(u32::from_be_bytes(b.get(at..at + 4).ok_or_else(t)?.try_into().map_err(|_| t())?)) };
        let spi = word(4)?;
        let seq = word(8)?;
        if spi < 256 {
            // SPI values 1-255 are reserved by IANA and 0 is never sent (RFC 4302 §2.4).
            return Err(WireError::Unsupported("ah", format!("reserved SPI {spi}")));
        }
        Ok(Ah { next, spi, seq, icv: b.get(FIXED..head).ok_or_else(t)?.to_vec(), payload: b.get(head..).ok_or_else(t)?.to_vec() })
    }

    /// The ICV over `ip` (an IPv4 packet whose payload is this AH header with a zero ICV
    /// followed by the protected data), as §3.3.3 has it: mutable header fields zeroed.
    fn icv_over(integ: Integ, key: &[u8], ip: &Ipv4) -> Vec<u8> {
        let mut image = ip.encode();
        // TOS (byte 1), flags and fragment offset (6, 7), TTL (8) and checksum (10, 11).
        for at in [1usize, 6, 7, 8, 10, 11] {
            if let Some(b) = image.get_mut(at) {
                *b = 0;
            }
        }
        integ.mac(key, &image)
    }

    /// Tunnel mode (§3.1.1): `outer` is the new IPv4 header (its protocol is set to 51 and its
    /// payload replaced); the whole `inner` packet (protocol `next`, 4 for IPv4) follows the AH
    /// header. The returned packet carries the ICV.
    pub fn protect(integ: Integ, key: &[u8], mut outer: Ipv4, spi: u32, seq: u32, next: u8, inner: &[u8]) -> Ipv4 {
        outer.protocol = PROTO_AH;
        let mut ah = Ah { next, spi, seq, icv: vec![0; integ.icv_len()], payload: inner.to_vec() };
        outer.payload = ah.encode();
        ah.icv = Ah::icv_over(integ, key, &outer);
        outer.payload = ah.encode();
        outer
    }

    /// Transport mode (§3.1.1): AH goes between the original IPv4 header and its payload; the
    /// next header is the original protocol.
    pub fn protect_transport(integ: Integ, key: &[u8], packet: &Ipv4, spi: u32, seq: u32) -> Ipv4 {
        Ah::protect(integ, key, Ipv4 { payload: Vec::new(), ..packet.clone() }, spi, seq, packet.protocol, &packet.payload)
    }

    /// Checks the ICV of a received AH packet `ip` (protocol 51, this header as its payload)
    /// with `key`: the ICV is recomputed over the packet with the ICV field zeroed and
    /// compared, every byte. `false` for a wrong length of ICV, key, or a tampered packet.
    pub fn verify(&self, integ: Integ, key: &[u8], ip: &Ipv4) -> bool {
        if ip.protocol != PROTO_AH || self.icv.len() != integ.icv_len() {
            return false;
        }
        let zeroed = Ah { icv: vec![0; self.icv.len()], ..self.clone() };
        let image = Ipv4 { payload: zeroed.encode(), ..ip.clone() };
        let want = Ah::icv_over(integ, key, &image);
        want.len() == self.icv.len() && want.iter().zip(&self.icv).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }

    /// Opens a received tunnel-mode AH packet: the inner packet when the ICV checks out and
    /// the next header says IPv4.
    pub fn open_tunnel(ip: &Ipv4, integ: Integ, key: &[u8]) -> Result<Vec<u8>> {
        let ah = Ah::decode(&ip.payload)?;
        if !ah.verify(integ, key, ip) {
            return Err(WireError::Checksum("ah integrity"));
        }
        if ah.next != crate::vpn::NEXT_IPV4 {
            return Err(WireError::Unsupported("ah", format!("next header {}", ah.next)));
        }
        Ok(ah.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha1::hmac_sha1;
    use std::net::Ipv4Addr;

    fn outer() -> Ipv4 {
        Ipv4 { id: 0x1234, ttl: 255, protocol: 0, src: Ipv4Addr::new(198, 51, 100, 1), dst: Ipv4Addr::new(203, 0, 113, 1), payload: Vec::new() }
    }

    fn inner() -> Vec<u8> {
        Ipv4 { id: 7, ttl: 64, protocol: 1, src: Ipv4Addr::new(192, 168, 1, 10), dst: Ipv4Addr::new(192, 168, 2, 10), payload: vec![8, 0, 1, 2] }
            .encode()
    }

    #[test]
    fn tunnel_mode_matches_the_layout_and_icv_of_rfc_4302() {
        let key = [0x0bu8; 20];
        let inner = inner();
        let p = Ah::protect(Integ::HmacSha1_96, &key, outer(), 0x0000_0abc, 5, 4, &inner);
        assert_eq!(p.protocol, 51);
        // AH header: next header 4, payload length 4 (24 bytes = 6 words - 2), reserved 0,
        // SPI, sequence number, 12 bytes of ICV, then the inner packet.
        assert_eq!(&p.payload[..12], &[4, 4, 0, 0, 0, 0, 0x0a, 0xbc, 0, 0, 0, 5]);
        assert_eq!(&p.payload[24..], &inner[..]);
        // The ICV by hand: the outer header with TOS, flags/fragment, TTL and checksum zeroed,
        // the AH header with a zero ICV, the inner packet.
        let total = u16::try_from(20 + 24 + inner.len()).unwrap();
        let mut img = vec![0x45, 0, 0, 0, 0x12, 0x34, 0, 0, 0, 51, 0, 0, 198, 51, 100, 1, 203, 0, 113, 1];
        img[2..4].copy_from_slice(&total.to_be_bytes());
        img.extend_from_slice(&[4, 4, 0, 0, 0, 0, 0x0a, 0xbc, 0, 0, 0, 5]);
        img.extend_from_slice(&[0; 12]);
        img.extend_from_slice(&inner);
        assert_eq!(&p.payload[12..24], &hmac_sha1(&key, &img)[..12]);
        // And it verifies, after a trip through the wire format.
        let back = Ipv4::decode(&p.encode()).unwrap();
        assert_eq!(Ah::open_tunnel(&back, Integ::HmacSha1_96, &key).unwrap(), inner);
    }

    #[test]
    fn mutable_fields_do_not_matter_and_immutable_ones_do() {
        let key = [3u8; 32];
        for integ in [Integ::HmacSha1_96, Integ::HmacMd5_96, Integ::HmacSha256_128] {
            let k = &key[..integ.key_len()];
            let p = Ah::protect(integ, k, outer(), 0x300, 1, 4, &inner());
            let ok = |q: &Ipv4| Ah::open_tunnel(q, integ, k).is_ok();
            assert!(ok(&p));
            // A router on the way changes the TTL, the DS field, the DF flag and the checksum.
            let mut raw = p.encode();
            raw[1] = 0xb8;
            raw[6] = 0x00;
            raw[8] = 3;
            let q = Ipv4::decode(&{
                let mut r = raw.clone();
                r[10] = 0;
                r[11] = 0;
                let c = crate::checksum(&[&r[..20]]);
                r[10..12].copy_from_slice(&c.to_be_bytes());
                r
            })
            .unwrap();
            assert!(ok(&q), "{}", integ.name());
            // Covered: identification, addresses, protocol, SPI, sequence, the payload, the key.
            assert!(!ok(&Ipv4 { id: p.id ^ 1, ..p.clone() }));
            assert!(!ok(&Ipv4 { src: Ipv4Addr::new(1, 1, 1, 1), ..p.clone() }));
            assert!(!ok(&Ipv4 { dst: Ipv4Addr::new(1, 1, 1, 1), ..p.clone() }));
            let mut bad = p.clone();
            *bad.payload.last_mut().unwrap() ^= 1;
            assert!(!ok(&bad));
            let mut bad = p.clone();
            bad.payload[7] ^= 1;
            assert!(!ok(&bad), "spi");
            let mut bad = p.clone();
            bad.payload[11] ^= 1;
            assert!(!ok(&bad), "seq");
            assert!(Ah::open_tunnel(&p, integ, &vec![9; integ.key_len()]).is_err());
            // Another suite's ICV length is refused outright.
            let other = if integ == Integ::HmacSha256_128 { Integ::HmacSha1_96 } else { Integ::HmacSha256_128 };
            assert!(Ah::open_tunnel(&p, other, &vec![3; other.key_len()]).is_err());
        }
    }

    #[test]
    fn sha256_uses_a_128_bit_icv_and_transport_mode_keeps_the_header() {
        let key = [5u8; 32];
        let p = Ah::protect(Integ::HmacSha256_128, &key, outer(), 0x400, 9, 4, &inner());
        // 12 + 16 bytes = 7 words, payload length 5.
        assert_eq!(p.payload[1], 5);
        assert_eq!(Ah::decode(&p.payload).unwrap().icv.len(), 16);
        // Transport mode: the original header stays, AH sits after it with the original
        // protocol as its next header, and the payload follows untouched.
        let pkt =
            Ipv4 { id: 9, ttl: 64, protocol: 47, src: Ipv4Addr::new(198, 51, 100, 1), dst: Ipv4Addr::new(203, 0, 113, 1), payload: vec![9; 30] };
        let t = Ah::protect_transport(Integ::HmacSha1_96, &key[..20], &pkt, 0x500, 2);
        assert_eq!((t.protocol, t.id, t.src, t.dst), (PROTO_AH, 9, pkt.src, pkt.dst));
        let ah = Ah::decode(&t.payload).unwrap();
        assert_eq!((ah.next, ah.payload.clone()), (47, pkt.payload.clone()));
        assert!(ah.verify(Integ::HmacSha1_96, &key[..20], &Ipv4::decode(&t.encode()).unwrap()));
        let mut forged = t.clone();
        *forged.payload.last_mut().unwrap() ^= 1;
        assert!(!Ah::decode(&forged.payload).unwrap().verify(Integ::HmacSha1_96, &key[..20], &forged));
        // Tunnel opening refuses a transport-mode packet (next header 47).
        assert!(Ah::open_tunnel(&t, Integ::HmacSha1_96, &key[..20]).is_err());
    }

    #[test]
    fn hostile_headers_are_refused_without_panicking() {
        let key = [1u8; 20];
        let p = Ah::protect(Integ::HmacSha1_96, &key, outer(), 0x300, 1, 4, &inner());
        let raw = p.payload.clone();
        for n in 0..raw.len() {
            let _ = Ah::decode(&raw[..n]);
            let _ = Ah::open_tunnel(&Ipv4 { payload: raw[..n].to_vec(), ..p.clone() }, Integ::HmacSha1_96, &key);
        }
        assert!(Ah::decode(&[4, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]).is_err(), "payload length 0 is an 8-byte header: too short for the SPI check");
        assert!(Ah::decode(&[4, 255, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]).is_err(), "an enormous ICV");
        let mut reserved = raw.clone();
        reserved[4..8].copy_from_slice(&7u32.to_be_bytes());
        assert!(Ah::decode(&reserved).is_err(), "reserved SPI");
        // A header whose length claims more than the packet holds.
        assert!(Ah::decode(&[4, 4, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 2]).is_err());
    }
}
