//! PEAP (EAP method 25) as the simulator carries it: a real TLS 1.2 session (`crate::tls`, the
//! same handshake a router's secure web server speaks, with the server's X.509 certificate chain
//! and a Diffie-Hellman exchange in the 2048-bit RFC 3526 group) inside EAP messages, and an
//! inner EAP login inside the tunnel.
//!
//! What this module holds is what is PEAP's own:
//!
//! - The message layout (RFC 5216 §3.1): a flags byte (length included, more fragments, start,
//!   version 0), a four-byte total length on the first fragment of a fragmented message, and the
//!   TLS bytes. [`fragments`] cuts a TLS flight into messages of at most [`FRAGMENT`] bytes, the
//!   receiver acknowledges each one but the last with an empty response ([`ack`]), and
//!   [`reassemble`] puts the flight back together.
//! - The key the tunnel exports, `PRF(master secret, "client EAP encryption", client random |
//!   server random)` (RFC 5216 §2.3), whose first 32 bytes are the pairwise master key (see
//!   `Tls::export`).
//! - The inner method, EAP-GTC (RFC 3748 §5.6: the password in the clear, which is why it needs
//!   the tunnel) after an inner identity exchange, and its result as a PEAPv0 Result TLV (EAP
//!   type 33, TLV type 3: 1 success, 2 failure).
//!
//! Left out: PEAPv1 and cryptobinding, session resumption, client certificates, and inner
//! methods other than GTC.

use crate::{Result, WireError, be16, tls};

/// PEAP's data flags: length included, more fragments, start; the low bits are the version (0).
pub const FLAG_LENGTH: u8 = 0x80;
pub const FLAG_MORE: u8 = 0x40;
pub const FLAG_START: u8 = 0x20;

pub const EAP_GTC: u8 = 6;
pub const EAP_TLV: u8 = 33;

pub const TLV_RESULT: u16 = 3;
pub const RESULT_SUCCESS: u16 = 1;
pub const RESULT_FAILURE: u16 = 2;

/// The label of the exported key (RFC 5216 §2.3).
pub const KEY_LABEL: &str = "client EAP encryption";

/// Most TLS bytes in one message (the rest of a longer flight follows in later messages).
pub const FRAGMENT: usize = 900;
/// Most TLS bytes of one reassembled flight.
pub const MAX_FLIGHT: usize = 32 * 1024;
/// Most TLVs read from one inner message.
const MAX_TLVS: usize = 16;

fn t() -> WireError {
    WireError::Truncated("peap")
}

/// One PEAP message: its flags, the total length it announces (first fragment of a fragmented
/// message) and its TLS bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part<'a> {
    pub flags: u8,
    pub total: Option<usize>,
    pub tls: &'a [u8],
}

/// A message with these flags, the announced total length when `total` is given, and TLS bytes.
pub fn message(flags: u8, total: Option<usize>, tls: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(tls.len() + 5);
    match total {
        Some(n) => {
            v.push(flags | FLAG_LENGTH);
            v.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_be_bytes());
        }
        None => v.push(flags & !FLAG_LENGTH),
    }
    v.extend_from_slice(tls);
    v
}

/// The server's start message: no TLS bytes yet.
pub fn start() -> Vec<u8> {
    vec![FLAG_START]
}

/// The empty response that acknowledges a fragment.
pub fn ack() -> Vec<u8> {
    vec![0]
}

/// The parts of a message's data.
pub fn parse(data: &[u8]) -> Result<Part<'_>> {
    let flags = *data.first().ok_or_else(t)?;
    if flags & FLAG_LENGTH != 0 {
        let total = data.get(1..5).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes).ok_or_else(t)?;
        let total = usize::try_from(total).unwrap_or(usize::MAX);
        if total > MAX_FLIGHT {
            return Err(WireError::Unsupported("peap", format!("a flight of {total} bytes")));
        }
        Ok(Part { flags, total: Some(total), tls: data.get(5..).ok_or_else(t)? })
    } else {
        Ok(Part { flags, total: None, tls: data.get(1..).ok_or_else(t)? })
    }
}

/// A TLS flight as messages: one when it fits in [`FRAGMENT`] bytes, else the first with the
/// length and the more-fragments flag, then the rest.
pub fn fragments(tls: &[u8]) -> Vec<Vec<u8>> {
    if tls.len() <= FRAGMENT {
        return vec![message(0, None, tls)];
    }
    let pieces: Vec<&[u8]> = tls.chunks(FRAGMENT).collect();
    let last = pieces.len().saturating_sub(1);
    pieces
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let more = if i < last { FLAG_MORE } else { 0 };
            message(more, (i == 0).then_some(tls.len()), p)
        })
        .collect()
}

/// Adds a message's TLS bytes to what has arrived of a fragmented flight; the whole flight when
/// this was the last fragment, `None` while more are to come.
pub fn reassemble(buf: &mut Vec<u8>, part: &Part<'_>) -> Result<Option<Vec<u8>>> {
    if buf.len().saturating_add(part.tls.len()) > MAX_FLIGHT {
        buf.clear();
        return Err(WireError::Unsupported("peap", "a flight that is too long".into()));
    }
    buf.extend_from_slice(part.tls);
    if part.flags & FLAG_MORE != 0 { Ok(None) } else { Ok(Some(std::mem::take(buf))) }
}

/// A Result TLV (PEAPv0 inner EAP type 33): mandatory, type 3, length 2, the result.
pub fn result_tlv(ok: bool) -> Vec<u8> {
    let mut v = (0x8000 | TLV_RESULT).to_be_bytes().to_vec();
    v.extend_from_slice(&2u16.to_be_bytes());
    v.extend_from_slice(&(if ok { RESULT_SUCCESS } else { RESULT_FAILURE }).to_be_bytes());
    v
}

/// The result a TLV list carries, when it has a Result TLV.
pub fn tlv_result(b: &[u8]) -> Option<bool> {
    let mut at = 0;
    for _ in 0..MAX_TLVS {
        let kind = be16(b, at)? & 0x3fff;
        let len = usize::from(be16(b, at + 2)?);
        if kind == TLV_RESULT {
            return Some(be16(b, at + 4)? == RESULT_SUCCESS);
        }
        at += 4 + len;
    }
    None
}

/// What a PEAP message shows on the wire, for the event list.
pub fn describe(data: &[u8]) -> String {
    match parse(data) {
        Ok(p) if p.tls.is_empty() => {
            if p.flags & FLAG_START != 0 {
                "start".into()
            } else {
                "acknowledgement".into()
            }
        }
        Ok(p) if p.flags & FLAG_MORE != 0 => match p.total {
            Some(n) => format!("TLS fragment, first {} of {n} bytes", p.tls.len()),
            None => format!("TLS fragment, {} bytes, more to come", p.tls.len()),
        },
        Ok(p) => tls::describe(p.tls),
        Err(e) => format!("unreadable ({e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_flight_is_one_message_and_a_long_one_is_cut_and_put_back() {
        let short = vec![7u8; 300];
        assert_eq!(fragments(&short), vec![[&[0u8][..], &short].concat()]);
        let long: Vec<u8> = (0..2500u32).map(|i| (i % 251) as u8).collect();
        let parts = fragments(&long);
        assert_eq!(parts.len(), 3);
        let first = parse(&parts[0]).unwrap();
        assert_eq!((first.flags, first.total, first.tls.len()), (FLAG_LENGTH | FLAG_MORE, Some(2500), FRAGMENT));
        assert_eq!(parse(&parts[1]).unwrap().flags, FLAG_MORE);
        assert_eq!(parse(&parts[2]).unwrap().flags, 0);
        let mut buf = Vec::new();
        assert_eq!(reassemble(&mut buf, &parse(&parts[0]).unwrap()).unwrap(), None);
        assert_eq!(reassemble(&mut buf, &parse(&parts[1]).unwrap()).unwrap(), None);
        assert_eq!(reassemble(&mut buf, &parse(&parts[2]).unwrap()).unwrap(), Some(long));
        assert!(buf.is_empty());
        assert_eq!(fragments(&[1u8; FRAGMENT]).len(), 1);
        assert_eq!(fragments(&[1u8; FRAGMENT + 1]).len(), 2);
    }

    #[test]
    fn start_ack_and_describe() {
        assert_eq!(describe(&start()), "start");
        assert_eq!(describe(&ack()), "acknowledgement");
        let (_c, hello) = tls::Tls::client(b"peap describe");
        assert!(describe(&message(0, None, &hello)).contains("ClientHello"));
        let parts = fragments(&[0x16u8; 2000]);
        assert_eq!(describe(&parts[0]), "TLS fragment, first 900 of 2000 bytes");
        assert!(describe(&parts[1]).contains("more to come"));
        assert!(describe(&[]).starts_with("unreadable"));
    }

    #[test]
    fn hostile_messages_are_refused_not_fatal() {
        let data = fragments(&[5u8; 2000])[0].clone();
        for n in 0..data.len().min(8) {
            let _ = parse(&data[..n]);
            let _ = describe(&data[..n]);
        }
        assert!(parse(&message(FLAG_LENGTH, Some(10_000_000), b"x")).is_err());
        let mut buf = vec![0u8; MAX_FLIGHT];
        assert!(reassemble(&mut buf, &Part { flags: FLAG_MORE, total: None, tls: b"xx" }).is_err());
        assert!(buf.is_empty());
        assert_eq!(tlv_result(&result_tlv(true)), Some(true));
        assert_eq!(tlv_result(&result_tlv(false)), Some(false));
        assert_eq!(tlv_result(&[0, 1]), None);
        assert_eq!(tlv_result(&[]), None);
    }
}
