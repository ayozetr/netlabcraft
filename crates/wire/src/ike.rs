//! ISAKMP (RFC 2408) messages as IKEv1 (RFC 2409) uses them, over UDP port 500: the fixed
//! header (§3.1), the generic payload header (§3.2) and the payloads of main mode and quick
//! mode — security association with its proposals and transforms (§3.4–3.6, attributes in
//! RFC 2409 appendix A and RFC 2407 §4.5), key exchange, identification, hash, nonce and
//! notification.
//!
//! Messages after the keys are agreed (main mode 5 and 6, quick mode) travel encrypted: their
//! payloads follow the header as one opaque block: AES-, DES- or 3DES-CBC (RFC 2409
//! appendix B: the payloads padded to whole blocks, the last pad byte counting the others, the
//! IV chained from message to message), see [`Isakmp::sealed_cbc`]. [`Isakmp::sealed`] is an
//! older keyed XOR made from MD5, **not real encryption**, that nothing in the simulator uses
//! for IKE any more.

use crate::cipher::Cipher;
use crate::md5::md5;
use crate::{Result, WireError, be16};

/// UDP port of ISAKMP.
pub const ISAKMP_PORT: u16 = 500;
/// ISAKMP version 1.0.
const VERSION: u8 = 0x10;
const HEADER_LEN: usize = 28;
/// Exchange types.
pub const EXCH_MAIN: u8 = 2;
pub const EXCH_INFO: u8 = 5;
pub const EXCH_QUICK: u8 = 32;
/// Header flags.
pub const FLAG_ENCRYPTED: u8 = 0x01;
pub const FLAG_COMMIT: u8 = 0x02;
/// Payload types.
pub const P_SA: u8 = 1;
pub const P_PROPOSAL: u8 = 2;
pub const P_TRANSFORM: u8 = 3;
pub const P_KE: u8 = 4;
pub const P_ID: u8 = 5;
pub const P_HASH: u8 = 8;
pub const P_NONCE: u8 = 10;
pub const P_NOTIFY: u8 = 11;
pub const P_DELETE: u8 = 12;
/// Protocol identifiers in proposals.
pub const PROTO_ISAKMP: u8 = 1;
pub const PROTO_ESP: u8 = 3;
/// Notify message types.
pub const NO_PROPOSAL_CHOSEN: u16 = 14;
pub const INVALID_ID_INFORMATION: u16 = 18;
pub const INVALID_KEY_INFORMATION: u16 = 17;
pub const AUTHENTICATION_FAILED: u16 = 24;
/// Most payloads (and transforms) read from one message (hostile input).
const MAX_PAYLOADS: usize = 32;

/// The name of a notify message type.
pub fn notify_name(kind: u16) -> &'static str {
    match kind {
        NO_PROPOSAL_CHOSEN => "NO-PROPOSAL-CHOSEN",
        INVALID_ID_INFORMATION => "INVALID-ID-INFORMATION",
        AUTHENTICATION_FAILED => "AUTHENTICATION-FAILED",
        INVALID_KEY_INFORMATION => "INVALID-KEY-INFORMATION",
        _ => "notification",
    }
}

/// A transform: its number, its identifier and its attributes (type, value), all sent in the
/// short (TV) form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transform {
    pub number: u8,
    pub id: u8,
    pub attrs: Vec<(u16, u16)>,
}

/// A proposal: number, protocol, SPI and transforms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub number: u8,
    pub protocol: u8,
    pub spi: Vec<u8>,
    pub transforms: Vec<Transform>,
}

/// A payload of a message in the clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// Security association (IPsec DOI, identity-only situation) with its proposals.
    Sa(Vec<Proposal>),
    Ke(Vec<u8>),
    /// Identification: type, protocol, port, data.
    Id {
        kind: u8,
        protocol: u8,
        port: u16,
        data: Vec<u8>,
    },
    Hash(Vec<u8>),
    Nonce(Vec<u8>),
    Notify {
        protocol: u8,
        kind: u16,
        spi: Vec<u8>,
    },
    /// A payload this decoder doesn't look into.
    Other(u8, Vec<u8>),
}

impl Payload {
    fn kind(&self) -> u8 {
        match self {
            Payload::Sa(_) => P_SA,
            Payload::Ke(_) => P_KE,
            Payload::Id { .. } => P_ID,
            Payload::Hash(_) => P_HASH,
            Payload::Nonce(_) => P_NONCE,
            Payload::Notify { .. } => P_NOTIFY,
            Payload::Other(k, _) => *k,
        }
    }

    /// The payload's short name, as the event list shows it.
    pub fn name(&self) -> &'static str {
        match self {
            Payload::Sa(_) => "SA",
            Payload::Ke(_) => "KE",
            Payload::Id { .. } => "ID",
            Payload::Hash(_) => "HASH",
            Payload::Nonce(_) => "NONCE",
            Payload::Notify { .. } => "NOTIFY",
            Payload::Other(..) => "other",
        }
    }

    /// The payload's body (what follows its generic header), as HASH computations take it.
    pub fn body(&self) -> Vec<u8> {
        match self {
            Payload::Sa(props) => {
                let mut v = Vec::new();
                v.extend_from_slice(&1u32.to_be_bytes()); // DOI: IPsec
                v.extend_from_slice(&1u32.to_be_bytes()); // situation: identity only
                for (k, p) in props.iter().enumerate() {
                    let mut b = vec![p.number, p.protocol, u8::try_from(p.spi.len()).unwrap_or(0), u8::try_from(p.transforms.len()).unwrap_or(0)];
                    b.extend_from_slice(&p.spi);
                    for (j, t) in p.transforms.iter().enumerate() {
                        let mut tb = vec![t.number, t.id, 0, 0];
                        for (ty, val) in &t.attrs {
                            tb.extend_from_slice(&(0x8000 | ty).to_be_bytes());
                            tb.extend_from_slice(&val.to_be_bytes());
                        }
                        let next = if j + 1 < p.transforms.len() { P_TRANSFORM } else { 0 };
                        push_payload(&mut b, next, &tb);
                    }
                    let next = if k + 1 < props.len() { P_PROPOSAL } else { 0 };
                    push_payload(&mut v, next, &b);
                }
                v
            }
            Payload::Ke(d) | Payload::Hash(d) | Payload::Nonce(d) | Payload::Other(_, d) => d.clone(),
            Payload::Id { kind, protocol, port, data } => {
                let mut v = vec![*kind, *protocol];
                v.extend_from_slice(&port.to_be_bytes());
                v.extend_from_slice(data);
                v
            }
            Payload::Notify { protocol, kind, spi } => {
                let mut v = Vec::new();
                v.extend_from_slice(&1u32.to_be_bytes());
                v.push(*protocol);
                v.push(u8::try_from(spi.len()).unwrap_or(0));
                v.extend_from_slice(&kind.to_be_bytes());
                v.extend_from_slice(spi);
                v
            }
        }
    }

    fn decode(kind: u8, b: &[u8]) -> Result<Payload> {
        let t = || WireError::Truncated("isakmp");
        Ok(match kind {
            P_SA => {
                let mut props = Vec::new();
                let mut at = 8;
                let mut next = if b.len() > 8 { P_PROPOSAL } else { 0 };
                b.get(..8).ok_or_else(t)?;
                for _ in 0..MAX_PAYLOADS {
                    if next != P_PROPOSAL {
                        break;
                    }
                    let (n, body, len) = generic(b, at)?;
                    next = n;
                    at += len;
                    let (number, protocol, spi_len, count) = (
                        *body.first().ok_or_else(t)?,
                        *body.get(1).ok_or_else(t)?,
                        usize::from(*body.get(2).ok_or_else(t)?),
                        usize::from(*body.get(3).ok_or_else(t)?),
                    );
                    let spi = body.get(4..4 + spi_len).ok_or_else(t)?.to_vec();
                    let mut transforms = Vec::new();
                    let mut tat = 4 + spi_len;
                    for _ in 0..count.min(MAX_PAYLOADS) {
                        let (_, tb, tlen) = generic(body, tat)?;
                        tat += tlen;
                        let mut attrs = Vec::new();
                        let mut aat = 4;
                        while aat < tb.len() && attrs.len() < MAX_PAYLOADS {
                            let ty = be16(tb, aat).ok_or_else(t)?;
                            if ty & 0x8000 == 0 {
                                // A long (TLV) attribute: skipped.
                                let alen = usize::from(be16(tb, aat + 2).ok_or_else(t)?);
                                tb.get(aat + 4..aat + 4 + alen).ok_or_else(t)?;
                                aat += 4 + alen;
                                continue;
                            }
                            attrs.push((ty & 0x7fff, be16(tb, aat + 2).ok_or_else(t)?));
                            aat += 4;
                        }
                        transforms.push(Transform { number: *tb.first().ok_or_else(t)?, id: *tb.get(1).ok_or_else(t)?, attrs });
                    }
                    props.push(Proposal { number, protocol, spi, transforms });
                }
                Payload::Sa(props)
            }
            P_KE => Payload::Ke(b.to_vec()),
            P_HASH => Payload::Hash(b.to_vec()),
            P_NONCE => Payload::Nonce(b.to_vec()),
            P_ID => Payload::Id {
                kind: *b.first().ok_or_else(t)?,
                protocol: *b.get(1).ok_or_else(t)?,
                port: be16(b, 2).ok_or_else(t)?,
                data: b.get(4..).ok_or_else(t)?.to_vec(),
            },
            P_NOTIFY => {
                let spi_len = usize::from(*b.get(5).ok_or_else(t)?);
                Payload::Notify {
                    protocol: *b.get(4).ok_or_else(t)?,
                    kind: be16(b, 6).ok_or_else(t)?,
                    spi: b.get(8..8 + spi_len).ok_or_else(t)?.to_vec(),
                }
            }
            k => Payload::Other(k, b.to_vec()),
        })
    }
}

/// Appends a payload with its generic header.
fn push_payload(v: &mut Vec<u8>, next: u8, body: &[u8]) {
    v.push(next);
    v.push(0);
    v.extend_from_slice(&u16::try_from(body.len() + 4).unwrap_or(u16::MAX).to_be_bytes());
    v.extend_from_slice(body);
}

/// The generic payload header at `at`: (next payload, body, whole length).
fn generic(b: &[u8], at: usize) -> Result<(u8, &[u8], usize)> {
    let t = || WireError::Truncated("isakmp");
    let next = *b.get(at).ok_or_else(t)?;
    let len = usize::from(be16(b, at + 2).ok_or_else(t)?);
    if len < 4 {
        return Err(WireError::Unsupported("isakmp", format!("payload length {len}")));
    }
    Ok((next, b.get(at + 4..at + len).ok_or_else(t)?, len))
}

/// An ISAKMP message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Isakmp {
    pub icookie: [u8; 8],
    pub rcookie: [u8; 8],
    pub exchange: u8,
    pub flags: u8,
    pub message_id: u32,
    /// The payloads, when the message travels in the clear.
    pub payloads: Vec<Payload>,
    /// The encrypted block, when it doesn't.
    pub encrypted: Vec<u8>,
    /// The header's next-payload field of an encrypted message (the first payload inside);
    /// 0 for the stand-in cipher, which keeps it inside.
    pub next: u8,
}

impl Isakmp {
    /// A message in the clear.
    pub fn clear(icookie: [u8; 8], rcookie: [u8; 8], exchange: u8, message_id: u32, payloads: Vec<Payload>) -> Isakmp {
        Isakmp { icookie, rcookie, exchange, flags: 0, message_id, payloads, encrypted: Vec::new(), next: 0 }
    }

    /// A message whose payloads are encrypted with `key` (see the module notes).
    pub fn sealed(icookie: [u8; 8], rcookie: [u8; 8], exchange: u8, message_id: u32, payloads: &[Payload], key: &[u8]) -> Isakmp {
        let first = payloads.first().map_or(0, Payload::kind);
        let mut plain = vec![first];
        plain.extend_from_slice(&chain(payloads));
        let encrypted = keystream_xor(key, &icookie, message_id, &plain);
        Isakmp { icookie, rcookie, exchange, flags: FLAG_ENCRYPTED, message_id, payloads: Vec::new(), encrypted, next: 0 }
    }

    /// A message whose payloads are encrypted with a block cipher in CBC mode (AES, DES or 3DES)
    /// under `cipher` from `iv` (RFC 2409 appendix B); the next message's IV is its
    /// [`Isakmp::last_block`].
    pub fn sealed_cbc(icookie: [u8; 8], rcookie: [u8; 8], exchange: u8, message_id: u32, payloads: &[Payload], cipher: &Cipher, iv: &[u8]) -> Isakmp {
        let block = cipher.block_len();
        let mut plain = chain(payloads);
        // Always some padding: zeros, then a byte counting them.
        let pad = block - 1 - plain.len() % block;
        plain.extend(std::iter::repeat_n(0u8, pad));
        plain.push(u8::try_from(pad).unwrap_or(0));
        let encrypted = cipher.cbc_encrypt(iv, &plain);
        let next = payloads.first().map_or(0, Payload::kind);
        Isakmp { icookie, rcookie, exchange, flags: FLAG_ENCRYPTED, message_id, payloads: Vec::new(), encrypted, next }
    }

    /// The payloads of a message [`Isakmp::sealed_cbc`] made; an error when what comes out
    /// doesn't parse (the wrong key or IV). The caller checks the HASH payload.
    pub fn open_cbc(&self, cipher: &Cipher, iv: &[u8]) -> Result<Vec<Payload>> {
        if self.encrypted.is_empty() || !self.encrypted.len().is_multiple_of(cipher.block_len()) {
            return Err(WireError::Truncated("isakmp"));
        }
        let plain = cipher.cbc_decrypt(iv, &self.encrypted);
        let pad = usize::from(*plain.last().ok_or(WireError::Truncated("isakmp"))?);
        let body = plain.get(..plain.len().checked_sub(pad + 1).ok_or(WireError::Truncated("isakmp"))?).ok_or(WireError::Truncated("isakmp"))?;
        parse_chain(self.next, body)
    }

    /// The last cipher block (`block` bytes) of an encrypted message: the IV of the next one.
    pub fn last_block(&self, block: usize) -> Option<Vec<u8>> {
        self.encrypted.get(self.encrypted.len().checked_sub(block)?..).map(<[u8]>::to_vec)
    }

    /// The payloads of a message [`Isakmp::sealed`] made, without the check [`Isakmp::open`]
    /// makes (the caller checks the HASH payload).
    pub fn open_unchecked(&self, key: &[u8]) -> Result<Vec<Payload>> {
        let plain = keystream_xor(key, &self.icookie, self.message_id, &self.encrypted);
        let first = *plain.first().ok_or(WireError::Truncated("isakmp"))?;
        parse_chain(first, plain.get(1..).unwrap_or_default())
    }

    /// The payloads of an encrypted message, read with `key`; an error when the key is wrong
    /// (what comes out doesn't parse).
    pub fn open(&self, key: &[u8]) -> Result<Vec<Payload>> {
        let plain = keystream_xor(key, &self.icookie, self.message_id, &self.encrypted);
        let first = *plain.first().ok_or(WireError::Truncated("isakmp"))?;
        let body = plain.get(1..).unwrap_or_default();
        let list = parse_chain(first, body)?;
        // A wrong key leaves garbage that might still parse: the hash must be there.
        if !list.iter().any(|p| matches!(p, Payload::Hash(h) if h.len() == 20)) {
            return Err(WireError::Unsupported("isakmp", "could not be decrypted".into()));
        }
        Ok(list)
    }

    pub fn encode(&self) -> Vec<u8> {
        let (first, body) = if self.flags & FLAG_ENCRYPTED != 0 {
            (
                if self.next != 0 {
                    self.next
                } else if self.encrypted.is_empty() {
                    0
                } else {
                    P_HASH
                },
                self.encrypted.clone(),
            )
        } else {
            (self.payloads.first().map_or(0, Payload::kind), chain(&self.payloads))
        };
        let mut v = Vec::with_capacity(HEADER_LEN + body.len());
        v.extend_from_slice(&self.icookie);
        v.extend_from_slice(&self.rcookie);
        v.push(first);
        v.push(VERSION);
        v.push(self.exchange);
        v.push(self.flags);
        v.extend_from_slice(&self.message_id.to_be_bytes());
        v.extend_from_slice(&u32::try_from(HEADER_LEN + body.len()).unwrap_or(u32::MAX).to_be_bytes());
        v.extend_from_slice(&body);
        v
    }

    pub fn decode(b: &[u8]) -> Result<Isakmp> {
        let t = || WireError::Truncated("isakmp");
        let header = b.get(..HEADER_LEN).ok_or_else(t)?;
        let cookie = |at: usize| -> Result<[u8; 8]> { header.get(at..at + 8).ok_or_else(t)?.try_into().map_err(|_| t()) };
        let (icookie, rcookie) = (cookie(0)?, cookie(8)?);
        let first = *header.get(16).ok_or_else(t)?;
        let version = *header.get(17).ok_or_else(t)?;
        if version >> 4 != 1 {
            return Err(WireError::Unsupported("isakmp", format!("version {}.{}", version >> 4, version & 0xf)));
        }
        let exchange = *header.get(18).ok_or_else(t)?;
        let flags = *header.get(19).ok_or_else(t)?;
        let word = |at: usize| -> Result<u32> { Ok(u32::from_be_bytes(header.get(at..at + 4).ok_or_else(t)?.try_into().map_err(|_| t())?)) };
        let message_id = word(20)?;
        let len = usize::try_from(word(24)?).unwrap_or(usize::MAX);
        if len < HEADER_LEN {
            return Err(WireError::Unsupported("isakmp", format!("length {len}")));
        }
        let body = b.get(HEADER_LEN..len).ok_or_else(t)?;
        if flags & FLAG_ENCRYPTED != 0 {
            return Ok(Isakmp { icookie, rcookie, exchange, flags, message_id, payloads: Vec::new(), encrypted: body.to_vec(), next: first });
        }
        Ok(Isakmp { icookie, rcookie, exchange, flags, message_id, payloads: parse_chain(first, body)?, encrypted: Vec::new(), next: 0 })
    }

    /// The SA payload's first proposal, if any.
    pub fn proposal(&self) -> Option<&Proposal> {
        self.payloads.iter().find_map(|p| match p {
            Payload::Sa(list) => list.first(),
            _ => None,
        })
    }

    /// The notification carried, if any.
    pub fn notify(&self) -> Option<u16> {
        self.payloads.iter().find_map(|p| match p {
            Payload::Notify { kind, .. } => Some(*kind),
            _ => None,
        })
    }
}

/// Payloads one after another, each header naming the next.
pub fn chain(payloads: &[Payload]) -> Vec<u8> {
    let mut v = Vec::new();
    for (k, p) in payloads.iter().enumerate() {
        let next = payloads.get(k + 1).map_or(0, Payload::kind);
        push_payload(&mut v, next, &p.body());
    }
    v
}

fn parse_chain(first: u8, body: &[u8]) -> Result<Vec<Payload>> {
    let mut out = Vec::new();
    let mut next = first;
    let mut at = 0;
    for _ in 0..MAX_PAYLOADS {
        if next == 0 {
            return Ok(out);
        }
        let (n, b, len) = generic(body, at)?;
        out.push(Payload::decode(next, b)?);
        next = n;
        at += len;
    }
    Err(WireError::Unsupported("isakmp", "too many payloads".into()))
}

/// XOR with a keystream of MD5 blocks over the key, the initiator's cookie, the message ID and
/// a counter (see the module notes: not real encryption).
fn keystream_xor(key: &[u8], cookie: &[u8; 8], message_id: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for (n, block) in (0u32..).zip(data.chunks(16)) {
        let mut seed = key.to_vec();
        seed.extend_from_slice(cookie);
        seed.extend_from_slice(&message_id.to_be_bytes());
        seed.extend_from_slice(&n.to_be_bytes());
        let ks = md5(&seed);
        out.extend(block.iter().zip(ks.iter()).map(|(a, b)| a ^ b));
    }
    out
}

/// Fixed-length pseudo-random bytes from a seed (cookies, nonces, key-exchange values).
pub fn filler(seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut n = 0u32;
    while out.len() < len.min(4_096) {
        let mut s = seed.to_vec();
        s.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&md5(&s));
        n += 1;
    }
    out.truncate(len.min(4_096));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cipher::CipherKind;

    fn main_1() -> Isakmp {
        let t = Transform { number: 1, id: 1, attrs: vec![(1, 7), (14, 256), (2, 2), (3, 1), (4, 14), (11, 1), (12, 28_800)] };
        let p = Proposal { number: 1, protocol: PROTO_ISAKMP, spi: Vec::new(), transforms: vec![t.clone(), Transform { number: 2, ..t }] };
        Isakmp::clear([1; 8], [0; 8], EXCH_MAIN, 0, vec![Payload::Sa(vec![p]), Payload::Other(13, vec![9; 16])])
    }

    #[test]
    fn messages_round_trip_and_survive_truncation() {
        let m = main_1();
        let v = m.encode();
        assert_eq!(Isakmp::decode(&v).unwrap(), m);
        assert_eq!(m.proposal().unwrap().transforms.len(), 2);
        for n in 0..v.len() {
            assert!(Isakmp::decode(&v[..n]).is_err(), "{n} bytes");
        }
        let ke = Isakmp::clear([1; 8], [2; 8], EXCH_MAIN, 0, vec![Payload::Ke(filler(b"x", 256)), Payload::Nonce(filler(b"y", 16))]);
        assert_eq!(Isakmp::decode(&ke.encode()).unwrap(), ke);
        let n = Isakmp::clear([1; 8], [2; 8], EXCH_INFO, 7, vec![Payload::Notify { protocol: PROTO_ISAKMP, kind: NO_PROPOSAL_CHOSEN, spi: vec![] }]);
        let back = Isakmp::decode(&n.encode()).unwrap();
        assert_eq!(back.notify(), Some(NO_PROPOSAL_CHOSEN));
        // A hostile payload length is refused.
        let mut bad = v.clone();
        bad[HEADER_LEN + 2] = 0;
        bad[HEADER_LEN + 3] = 2;
        assert!(Isakmp::decode(&bad).is_err());
    }

    #[test]
    fn encrypted_messages_open_only_with_the_right_key() {
        let payloads = [Payload::Id { kind: 1, protocol: 17, port: 500, data: vec![10, 0, 0, 1] }, Payload::Hash(filler(b"h", 20))];
        let m = Isakmp::sealed([3; 8], [4; 8], EXCH_MAIN, 0, &payloads, b"secret");
        let v = m.encode();
        let back = Isakmp::decode(&v).unwrap();
        assert_eq!(back.flags & FLAG_ENCRYPTED, FLAG_ENCRYPTED);
        assert_eq!(back.open(b"secret").unwrap(), payloads.to_vec());
        assert!(back.open(b"wrong").is_err());
        // The identity isn't readable on the wire.
        assert!(!v.windows(4).any(|w| w == [10, 0, 0, 1]));
    }

    #[test]
    fn cbc_messages_chain_their_iv_and_open_only_with_the_right_key() {
        let aes = Cipher::new(CipherKind::Aes, &[5; 32]).unwrap();
        let payloads = [Payload::Id { kind: 1, protocol: 17, port: 500, data: vec![10, 0, 0, 1] }, Payload::Hash(filler(b"h", 32))];
        let m = Isakmp::sealed_cbc([3; 8], [4; 8], EXCH_MAIN, 0, &payloads, &aes, &[9; 16]);
        assert_eq!(m.encrypted.len() % 16, 0);
        let v = m.encode();
        // The header names the first payload inside (ID), as RFC 2408 has it.
        assert_eq!(v[16], P_ID);
        let back = Isakmp::decode(&v).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.open_cbc(&aes, &[9; 16]).unwrap(), payloads.to_vec());
        assert!(!v.windows(4).any(|w| w == [10, 0, 0, 1]));
        assert_eq!(back.last_block(16).unwrap(), v[v.len() - 16..].to_vec());
        // Another key or IV gives garbage that doesn't parse back to the payloads.
        let other = Cipher::new(CipherKind::Aes, &[6; 32]).unwrap();
        assert_ne!(back.open_cbc(&other, &[9; 16]).ok(), Some(payloads.to_vec()));
        assert_ne!(back.open_cbc(&aes, &[8; 16]).ok(), Some(payloads.to_vec()));
        for n in 0..v.len() {
            if let Ok(x) = Isakmp::decode(&v[..n]) {
                let _ = x.open_cbc(&aes, &[9; 16]);
            }
        }
        let st = Isakmp::sealed([3; 8], [4; 8], EXCH_MAIN, 0, &payloads, b"k");
        assert_eq!(st.open_unchecked(b"k").unwrap(), payloads.to_vec());
        assert!(Isakmp { encrypted: vec![1; 7], ..m.clone() }.open_cbc(&aes, &[0; 16]).is_err());
    }

    #[test]
    fn des_and_3des_messages_pad_to_eight_byte_blocks_and_chain() {
        let payloads = [Payload::Id { kind: 1, protocol: 17, port: 500, data: vec![10, 0, 0, 1] }, Payload::Hash(filler(b"h", 20))];
        for (kind, key) in [(CipherKind::Des, vec![5u8; 8]), (CipherKind::Tdes, vec![6u8; 24])] {
            let c = Cipher::new(kind, &key).unwrap();
            let m = Isakmp::sealed_cbc([3; 8], [4; 8], EXCH_MAIN, 0, &payloads, &c, &[9; 8]);
            assert_eq!(m.encrypted.len() % 8, 0);
            let v = m.encode();
            assert!(!v.windows(4).any(|w| w == [10, 0, 0, 1]));
            let back = Isakmp::decode(&v).unwrap();
            assert_eq!(back.open_cbc(&c, &[9; 8]).unwrap(), payloads.to_vec());
            assert_eq!(back.last_block(8).unwrap(), v[v.len() - 8..].to_vec());
            assert_ne!(back.open_cbc(&c, &[8; 8]).ok(), Some(payloads.to_vec()));
            for n in 0..v.len() {
                if let Ok(x) = Isakmp::decode(&v[..n]) {
                    let _ = x.open_cbc(&c, &[9; 8]);
                }
            }
        }
    }
}
