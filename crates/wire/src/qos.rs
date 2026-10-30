//! The IPv4 DS field (RFC 2474): the six-bit DSCP that marks a packet's class of service,
//! followed by the two ECN bits (RFC 3168). Marking rewrites the field in place and fixes the
//! header checksum, so the change is visible in a capture.

use crate::checksum;

/// Highest DSCP value (six bits).
pub const MAX_DSCP: u8 = 63;

/// The named DSCP values: class selectors (RFC 2474), assured forwarding (RFC 2597) and
/// expedited forwarding (RFC 3246).
pub const DSCP_NAMES: [(&str, u8); 22] = [
    ("default", 0),
    ("cs1", 8),
    ("af11", 10),
    ("af12", 12),
    ("af13", 14),
    ("cs2", 16),
    ("af21", 18),
    ("af22", 20),
    ("af23", 22),
    ("cs3", 24),
    ("af31", 26),
    ("af32", 28),
    ("af33", 30),
    ("cs4", 32),
    ("af41", 34),
    ("af42", 36),
    ("af43", 38),
    ("cs5", 40),
    ("ef", 46),
    ("cs6", 48),
    ("cs7", 56),
    ("cs0", 0),
];

/// A DSCP given by name (`ef`, `af31`, `cs5`, `default`) or number (0-63).
pub fn parse_dscp(text: &str) -> Option<u8> {
    let t = text.trim().to_ascii_lowercase();
    if let Some((_, v)) = DSCP_NAMES.iter().find(|(n, _)| *n == t) {
        return Some(*v);
    }
    t.parse::<u8>().ok().filter(|v| *v <= MAX_DSCP)
}

/// How a DSCP value is written: its name when it has one, else its number.
pub fn dscp_name(v: u8) -> String {
    DSCP_NAMES.iter().find(|(_, x)| *x == v).map_or_else(|| v.to_string(), |(n, _)| (*n).to_string())
}

/// The IP header's length when `datagram` starts with a whole IPv4 header.
fn header_len(datagram: &[u8]) -> Option<usize> {
    let vihl = *datagram.first()?;
    if vihl >> 4 != 4 {
        return None;
    }
    let ihl = usize::from(vihl & 0x0f) * 4;
    (ihl >= 20 && datagram.len() >= ihl).then_some(ihl)
}

/// The whole DS byte (DSCP and ECN) of an IPv4 datagram.
pub fn ds_byte(datagram: &[u8]) -> Option<u8> {
    header_len(datagram)?;
    datagram.get(1).copied()
}

/// The DSCP of an IPv4 datagram.
pub fn dscp(datagram: &[u8]) -> Option<u8> {
    ds_byte(datagram).map(|b| b >> 2)
}

/// The IP precedence (the DSCP's top three bits, RFC 791's precedence field).
pub fn precedence(datagram: &[u8]) -> Option<u8> {
    dscp(datagram).map(|d| d >> 3)
}

/// Sets the whole DS byte and recomputes the header checksum. `false` (nothing changed) when
/// `datagram` is not an IPv4 packet.
pub fn set_ds_byte(datagram: &mut [u8], ds: u8) -> bool {
    let Some(ihl) = header_len(datagram) else { return false };
    let Some(b) = datagram.get_mut(1) else { return false };
    *b = ds;
    if let Some(c) = datagram.get_mut(10..12) {
        c.copy_from_slice(&[0, 0]);
    }
    let sum = datagram.get(..ihl).map_or(0, |h| checksum(&[h]));
    if let Some(c) = datagram.get_mut(10..12) {
        c.copy_from_slice(&sum.to_be_bytes());
    }
    true
}

/// Sets the DSCP (keeping the ECN bits) and recomputes the header checksum.
pub fn set_dscp(datagram: &mut [u8], value: u8) -> bool {
    let ecn = ds_byte(datagram).unwrap_or(0) & 0x03;
    set_ds_byte(datagram, ((value & MAX_DSCP) << 2) | ecn)
}

/// Sets the IP precedence: the DSCP's top three bits, the others kept.
pub fn set_precedence(datagram: &mut [u8], value: u8) -> bool {
    let low = dscp(datagram).unwrap_or(0) & 0x07;
    set_dscp(datagram, ((value & 0x07) << 3) | low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ipv4, PROTO_ICMP};
    use std::net::Ipv4Addr;

    fn packet() -> Vec<u8> {
        Ipv4 { id: 7, ttl: 64, protocol: PROTO_ICMP, src: Ipv4Addr::new(10, 0, 0, 1), dst: Ipv4Addr::new(10, 0, 0, 2), payload: vec![1, 2, 3, 4] }
            .encode()
    }

    #[test]
    fn marking_rewrites_the_ds_field_and_keeps_the_checksum_valid() {
        let mut p = packet();
        assert_eq!(dscp(&p), Some(0));
        assert!(set_dscp(&mut p, 46));
        assert_eq!(p.get(1), Some(&0xb8));
        assert_eq!(dscp(&p), Some(46));
        assert_eq!(precedence(&p), Some(5));
        // The header still checks (decode verifies the checksum) and nothing else moved.
        let ip = Ipv4::decode(&p).unwrap();
        assert_eq!((ip.id, ip.ttl, ip.payload.as_slice()), (7, 64, &[1u8, 2, 3, 4][..]));
        assert_eq!(checksum(&[&p[..20]]), 0);
        // ECN bits survive a DSCP change; precedence keeps the low DSCP bits.
        assert!(set_ds_byte(&mut p, (10 << 2) | 0x01));
        assert!(set_dscp(&mut p, 26));
        assert_eq!(p.get(1), Some(&((26 << 2) | 0x01)));
        assert!(set_precedence(&mut p, 1));
        assert_eq!(dscp(&p), Some(10));
        assert!(Ipv4::decode(&p).is_ok());
    }

    #[test]
    fn names_and_numbers() {
        assert_eq!(parse_dscp("EF"), Some(46));
        assert_eq!(parse_dscp("af41"), Some(34));
        assert_eq!(parse_dscp("cs0"), Some(0));
        assert_eq!(parse_dscp("default"), Some(0));
        assert_eq!(parse_dscp("63"), Some(63));
        assert_eq!(parse_dscp("64"), None);
        assert_eq!(parse_dscp("af14"), None);
        assert_eq!(dscp_name(46), "ef");
        assert_eq!(dscp_name(0), "default");
        assert_eq!(dscp_name(5), "5");
        for (n, v) in DSCP_NAMES {
            assert_eq!(parse_dscp(n), Some(v));
        }
    }

    #[test]
    fn hostile_bytes_are_left_alone() {
        let p = packet();
        for n in 0..p.len() {
            let mut cut = p[..n].to_vec();
            let _ = set_dscp(&mut cut, 46);
            let _ = dscp(&cut);
        }
        let mut v6 = vec![0x60; 40];
        assert!(!set_dscp(&mut v6, 46));
        assert_eq!(dscp(&v6), None);
        let mut short_ihl = packet();
        short_ihl[0] = 0x44;
        assert!(!set_dscp(&mut short_ihl, 1));
    }
}
