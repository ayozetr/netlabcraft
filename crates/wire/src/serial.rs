//! Framing on serial links: Cisco-style HDLC and PPP (RFC 1661, RFC 1662 without the flag
//! bytes, escaping and FCS, which belong to the physical layer here).
//!
//! HDLC: address `0x0F` (unicast), control `0x00`, then the Ethernet type of the payload.
//! PPP: address `0xFF`, control `0x03`, then a PPP protocol number (`0x0021` IPv4, `0x0057` IPv6).

use crate::ipv6::ETHERTYPE_IPV6;
use crate::{ETHERTYPE_IPV4, Result, WireError, be16};

const PPP_IPV4: u16 = 0x0021;
const PPP_IPV6: u16 = 0x0057;

/// The framing of a serial link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Hdlc,
    Ppp,
}

/// Wraps a packet of `ethertype` for the link. PPP carries IPv4 and IPv6 only.
pub fn encode(framing: Framing, ethertype: u16, payload: &[u8]) -> Result<Vec<u8>> {
    let head = match framing {
        Framing::Hdlc => [0x0f, 0x00, (ethertype >> 8) as u8, ethertype as u8],
        Framing::Ppp => {
            let proto = match ethertype {
                ETHERTYPE_IPV4 => PPP_IPV4,
                ETHERTYPE_IPV6 => PPP_IPV6,
                other => return Err(WireError::Unsupported("ppp", format!("ethertype 0x{other:04x}"))),
            };
            [0xff, 0x03, (proto >> 8) as u8, proto as u8]
        }
    };
    Ok([&head[..], payload].concat())
}

/// Unwraps a serial frame: its framing, the payload's Ethernet type, and the payload.
pub fn decode(b: &[u8]) -> Result<(Framing, u16, Vec<u8>)> {
    let t = || WireError::Truncated("serial");
    let kind = be16(b, 2).ok_or_else(t)?;
    let rest = b.get(4..).ok_or_else(t)?.to_vec();
    match (b[0], b[1]) {
        (0x0f | 0x8f, 0x00) => Ok((Framing::Hdlc, kind, rest)),
        (0xff, 0x03) => {
            let et = match kind {
                PPP_IPV4 => ETHERTYPE_IPV4,
                PPP_IPV6 => ETHERTYPE_IPV6,
                other => return Err(WireError::Unsupported("ppp", format!("protocol 0x{other:04x}"))),
            };
            Ok((Framing::Ppp, et, rest))
        }
        (a, c) => Err(WireError::Unsupported("serial", format!("address 0x{a:02x} control 0x{c:02x}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_framings_round_trip() {
        for f in [Framing::Hdlc, Framing::Ppp] {
            for et in [ETHERTYPE_IPV4, ETHERTYPE_IPV6] {
                let b = encode(f, et, &[1, 2, 3]).unwrap();
                assert_eq!(decode(&b).unwrap(), (f, et, vec![1, 2, 3]));
                for n in 0..b.len() {
                    let _ = decode(&b[..n]);
                }
            }
        }
        assert_eq!(&encode(Framing::Ppp, ETHERTYPE_IPV4, &[]).unwrap(), &[0xff, 0x03, 0x00, 0x21]);
        assert!(encode(Framing::Ppp, crate::ETHERTYPE_ARP, &[]).is_err());
        assert!(decode(&[0x45, 0, 0, 0, 0]).is_err());
    }
}
