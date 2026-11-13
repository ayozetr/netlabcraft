//! The attach exchange between a mobile device and the provider's central office server.
//!
//! This is a protocol of our own, not 3GPP signalling: a device that hears a cell tower sends
//! an *attach request* (carrying the access point name it wants), and the central office answers
//! with an *attach accept* (the device's address, prefix length and gateway) or an *attach
//! reject* (a reason code and a line of text). The messages ride in Ethernet frames of the IEEE
//! 802 "local experimental" ethertype 0x88B5, so they show in the event list and in captures
//! like any other frame.
//!
//! Layout: the four bytes `NLCA`, a version byte (1), a kind byte, then the body.
//!
//! | kind | body |
//! |---|---|
//! | 1 request | APN length (1 byte), APN |
//! | 2 accept | address (4), prefix length (1), gateway (4), APN length (1), APN |
//! | 3 reject | code (1), text length (1), text |

use std::net::Ipv4Addr;

use crate::{Result, WireError};

/// IEEE 802 local experimental ethertype 1.
pub const ETHERTYPE_CELL: u16 = 0x88b5;
const MAGIC: &[u8; 4] = b"NLCA";
const VERSION: u8 = 1;
/// Longest APN or reject text that is sent or read.
pub const MAX_TEXT: usize = 63;

/// Why the central office refused a device.
pub const REJECT_NO_POOL: u8 = 1;
pub const REJECT_PLAN: u8 = 2;
pub const REJECT_APN: u8 = 3;
pub const REJECT_EXHAUSTED: u8 = 4;
pub const REJECT_UNKNOWN: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attach {
    Request { apn: String },
    Accept { address: Ipv4Addr, prefix: u8, gateway: Ipv4Addr, apn: String },
    Reject { code: u8, text: String },
}

fn text(out: &mut Vec<u8>, s: &str) {
    let bytes: Vec<u8> = s.bytes().take(MAX_TEXT).collect();
    out.push(u8::try_from(bytes.len()).unwrap_or(0));
    out.extend_from_slice(&bytes);
}

fn read_text(b: &[u8], at: usize) -> Result<(String, usize)> {
    let t = || WireError::Truncated("cell attach");
    let len = usize::from(*b.get(at).ok_or_else(t)?);
    if len > MAX_TEXT {
        return Err(WireError::Unsupported("cell attach", "text too long".into()));
    }
    let end = at.checked_add(1).and_then(|s| s.checked_add(len)).ok_or_else(t)?;
    let raw = b.get(at + 1..end).ok_or_else(t)?;
    Ok((String::from_utf8_lossy(raw).into_owned(), end))
}

fn ip4(b: &[u8], at: usize) -> Result<Ipv4Addr> {
    let raw = b.get(at..at.saturating_add(4)).ok_or(WireError::Truncated("cell attach"))?;
    Ok(Ipv4Addr::new(
        raw.first().copied().unwrap_or(0),
        raw.get(1).copied().unwrap_or(0),
        raw.get(2).copied().unwrap_or(0),
        raw.get(3).copied().unwrap_or(0),
    ))
}

impl Attach {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(VERSION);
        match self {
            Attach::Request { apn } => {
                out.push(1);
                text(&mut out, apn);
            }
            Attach::Accept { address, prefix, gateway, apn } => {
                out.push(2);
                out.extend_from_slice(&address.octets());
                out.push(*prefix);
                out.extend_from_slice(&gateway.octets());
                text(&mut out, apn);
            }
            Attach::Reject { code, text: why } => {
                out.push(3);
                out.push(*code);
                text(&mut out, why);
            }
        }
        out
    }

    pub fn decode(b: &[u8]) -> Result<Attach> {
        let t = || WireError::Truncated("cell attach");
        if b.get(..4).ok_or_else(t)? != MAGIC {
            return Err(WireError::Unsupported("cell attach", "bad magic".into()));
        }
        let v = *b.get(4).ok_or_else(t)?;
        if v != VERSION {
            return Err(WireError::Unsupported("cell attach", format!("version {v}")));
        }
        match *b.get(5).ok_or_else(t)? {
            1 => Ok(Attach::Request { apn: read_text(b, 6)?.0 }),
            2 => {
                let address = ip4(b, 6)?;
                let prefix = *b.get(10).ok_or_else(t)?;
                if prefix > 32 {
                    return Err(WireError::Unsupported("cell attach", format!("prefix length {prefix}")));
                }
                let gateway = ip4(b, 11)?;
                Ok(Attach::Accept { address, prefix, gateway, apn: read_text(b, 15)?.0 })
            }
            3 => {
                let code = *b.get(6).ok_or_else(t)?;
                Ok(Attach::Reject { code, text: read_text(b, 7)?.0 })
            }
            k => Err(WireError::Unsupported("cell attach", format!("kind {k}"))),
        }
    }

    /// The event-list wording.
    pub fn describe(&self) -> String {
        match self {
            Attach::Request { apn } if apn.is_empty() => "Cell attach request (no APN)".to_string(),
            Attach::Request { apn } => format!("Cell attach request, APN {apn}"),
            Attach::Accept { address, prefix, gateway, .. } => format!("Cell attach accept: address {address}/{prefix}, gateway {gateway}"),
            Attach::Reject { code, text } => format!("Cell attach reject (code {code}): {text}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Attach> {
        vec![
            Attach::Request { apn: "internet".into() },
            Attach::Request { apn: String::new() },
            Attach::Accept { address: Ipv4Addr::new(10, 64, 0, 2), prefix: 24, gateway: Ipv4Addr::new(10, 64, 0, 1), apn: "internet".into() },
            Attach::Reject { code: REJECT_APN, text: "APN not accepted".into() },
        ]
    }

    #[test]
    fn every_message_round_trips() {
        for m in samples() {
            assert_eq!(Attach::decode(&m.encode()).unwrap(), m);
        }
    }

    #[test]
    fn every_truncation_and_hostile_byte_is_an_error_not_a_panic() {
        for m in samples() {
            let bytes = m.encode();
            for n in 0..bytes.len() {
                let _ = Attach::decode(&bytes[..n]);
            }
            for i in 0..bytes.len() {
                let mut x = bytes.clone();
                x[i] = x[i].wrapping_add(0x55);
                let _ = Attach::decode(&x);
            }
        }
        assert!(Attach::decode(&[]).is_err());
        assert!(Attach::decode(b"NLCA\x01\x02").is_err());
        assert!(Attach::decode(b"NLCB\x01\x01\x00").is_err());
        assert!(Attach::decode(b"NLCA\x01\x01\xff").is_err());
    }
}
