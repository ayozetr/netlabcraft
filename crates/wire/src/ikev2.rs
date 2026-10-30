//! IKEv2 messages (RFC 7296): the header, the payloads a site-to-site exchange uses (SA, KE,
//! nonce, identification, authentication, notify, traffic selectors) and the encrypted SK
//! payload, sealed with AES-128-CBC (RFC 3602) and HMAC-SHA1-96 (RFC 2404), with the key
//! derivation of RFC 7296 §2.13–2.15 (prf+ with HMAC-SHA1, the pre-shared key AUTH value).
//!
//! **The Diffie-Hellman exchange is a stand-in:** the KE payloads carry filler of the group's
//! size, and both sides take the "shared secret" from the two public KE payloads, so the keys
//! are not secret from someone who saw the exchange. Everything else (payload layout, the
//! cipher, the integrity check, AUTH over the signed octets) is as the RFC says.

use std::net::Ipv4Addr;

use crate::aes::{Aes128, cbc_decrypt, cbc_encrypt};
use crate::sha1::{hmac_sha1, sha1};
use crate::{Result, WireError};

/// IKEv2's major/minor version byte.
pub const VERSION: u8 = 0x20;
/// Exchange types.
pub const IKE_SA_INIT: u8 = 34;
pub const IKE_AUTH: u8 = 35;
pub const CREATE_CHILD_SA: u8 = 36;
pub const INFORMATIONAL: u8 = 37;
/// Header flags.
pub const FLAG_INITIATOR: u8 = 0x08;
pub const FLAG_RESPONSE: u8 = 0x20;
/// Payload types.
pub const PT_SA: u8 = 33;
pub const PT_KE: u8 = 34;
pub const PT_IDI: u8 = 35;
pub const PT_IDR: u8 = 36;
pub const PT_AUTH: u8 = 39;
pub const PT_NONCE: u8 = 40;
pub const PT_NOTIFY: u8 = 41;
pub const PT_TSI: u8 = 44;
pub const PT_TSR: u8 = 45;
pub const PT_SK: u8 = 46;
/// Security protocol ids in proposals.
pub const PROTO_IKE: u8 = 1;
pub const PROTO_ESP: u8 = 3;
/// Transform types and the ids used here.
pub const TT_ENCR: u8 = 1;
pub const TT_PRF: u8 = 2;
pub const TT_INTEG: u8 = 3;
pub const TT_DH: u8 = 4;
pub const TT_ESN: u8 = 5;
pub const ENCR_3DES: u16 = 3;
pub const ENCR_AES_CBC: u16 = 12;
pub const ENCR_NULL: u16 = 11;
pub const PRF_HMAC_MD5: u16 = 1;
pub const PRF_HMAC_SHA1: u16 = 2;
pub const PRF_HMAC_SHA2_256: u16 = 5;
pub const AUTH_HMAC_MD5_96: u16 = 1;
pub const AUTH_HMAC_SHA1_96: u16 = 2;
pub const AUTH_HMAC_SHA2_256_128: u16 = 12;
/// Notify message types.
pub const NO_PROPOSAL_CHOSEN: u16 = 14;
pub const INVALID_KE_PAYLOAD: u16 = 17;
pub const AUTHENTICATION_FAILED: u16 = 24;
pub const TS_UNACCEPTABLE: u16 = 38;
/// Identification types.
pub const ID_IPV4_ADDR: u8 = 1;
pub const ID_FQDN: u8 = 2;
/// Authentication method: shared key message integrity code.
pub const AUTH_SHARED_KEY: u8 = 2;
/// Traffic selector type: an IPv4 address range.
pub const TS_IPV4_ADDR_RANGE: u8 = 7;
/// The SK payload's cipher block and integrity check sizes (AES-CBC, HMAC-SHA1-96).
const BLOCK: usize = 16;
const ICV: usize = 12;
/// Longest message accepted (well beyond anything a lab sends).
pub const MAX_MESSAGE: usize = 16_384;
/// Most payloads, proposals or transforms read from one message.
const MAX_ITEMS: usize = 64;

/// A notify type's name for event lists.
pub fn notify_name(kind: u16) -> &'static str {
    match kind {
        NO_PROPOSAL_CHOSEN => "NO_PROPOSAL_CHOSEN",
        INVALID_KE_PAYLOAD => "INVALID_KE_PAYLOAD",
        AUTHENTICATION_FAILED => "AUTHENTICATION_FAILED",
        TS_UNACCEPTABLE => "TS_UNACCEPTABLE",
        _ => "notification",
    }
}

/// An exchange type's name.
pub fn exchange_name(e: u8) -> &'static str {
    match e {
        IKE_SA_INIT => "IKE_SA_INIT",
        IKE_AUTH => "IKE_AUTH",
        CREATE_CHILD_SA => "CREATE_CHILD_SA",
        INFORMATIONAL => "INFORMATIONAL",
        _ => "exchange",
    }
}

/// One transform: its type, id and key length (attribute 14), if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transform {
    pub kind: u8,
    pub id: u16,
    pub key_len: Option<u16>,
}

/// One proposal of an SA payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub number: u8,
    pub protocol: u8,
    pub spi: Vec<u8>,
    pub transforms: Vec<Transform>,
}

impl Proposal {
    /// The transforms of one type.
    pub fn of(&self, kind: u8) -> impl Iterator<Item = &Transform> {
        self.transforms.iter().filter(move |t| t.kind == kind)
    }
}

/// An IPv4 traffic selector (an address and port range of one IP protocol; 0: any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficSelector {
    pub protocol: u8,
    pub start_port: u16,
    pub end_port: u16,
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
}

/// A payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    Sa(Vec<Proposal>),
    Ke {
        group: u16,
        data: Vec<u8>,
    },
    IdI {
        kind: u8,
        data: Vec<u8>,
    },
    IdR {
        kind: u8,
        data: Vec<u8>,
    },
    Auth {
        method: u8,
        data: Vec<u8>,
    },
    Nonce(Vec<u8>),
    Notify {
        protocol: u8,
        spi: Vec<u8>,
        kind: u16,
        data: Vec<u8>,
    },
    TsI(Vec<TrafficSelector>),
    TsR(Vec<TrafficSelector>),
    /// The encrypted payload as it travels: the first inner payload's type and the body (IV,
    /// ciphertext, ICV).
    Sk {
        first: u8,
        body: Vec<u8>,
    },
    Other {
        kind: u8,
        body: Vec<u8>,
    },
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

fn len16(n: usize) -> [u8; 2] {
    u16::try_from(n).unwrap_or(u16::MAX).to_be_bytes()
}

impl Payload {
    pub fn kind(&self) -> u8 {
        match self {
            Payload::Sa(_) => PT_SA,
            Payload::Ke { .. } => PT_KE,
            Payload::IdI { .. } => PT_IDI,
            Payload::IdR { .. } => PT_IDR,
            Payload::Auth { .. } => PT_AUTH,
            Payload::Nonce(_) => PT_NONCE,
            Payload::Notify { .. } => PT_NOTIFY,
            Payload::TsI(_) => PT_TSI,
            Payload::TsR(_) => PT_TSR,
            Payload::Sk { .. } => PT_SK,
            Payload::Other { kind, .. } => *kind,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Payload::Sa(_) => "SA",
            Payload::Ke { .. } => "KE",
            Payload::IdI { .. } => "IDi",
            Payload::IdR { .. } => "IDr",
            Payload::Auth { .. } => "AUTH",
            Payload::Nonce(_) => "Nonce",
            Payload::Notify { .. } => "Notify",
            Payload::TsI(_) => "TSi",
            Payload::TsR(_) => "TSr",
            Payload::Sk { .. } => "SK",
            Payload::Other { .. } => "payload",
        }
    }

    /// The payload's body (what follows its generic header).
    pub fn body(&self) -> Vec<u8> {
        match self {
            Payload::Sa(props) => {
                let mut v = Vec::new();
                for (i, p) in props.iter().enumerate() {
                    let mut t = Vec::new();
                    for (j, tr) in p.transforms.iter().enumerate() {
                        let mut one = vec![if j + 1 == p.transforms.len() { 0 } else { 3 }, 0, 0, 0, tr.kind, 0];
                        one.extend_from_slice(&tr.id.to_be_bytes());
                        if let Some(k) = tr.key_len {
                            one.extend_from_slice(&(0x8000u16 | 14).to_be_bytes());
                            one.extend_from_slice(&k.to_be_bytes());
                        }
                        let l = len16(one.len());
                        one[2..4].copy_from_slice(&l);
                        t.extend(one);
                    }
                    let mut head = vec![if i + 1 == props.len() { 0 } else { 2 }, 0, 0, 0, p.number, p.protocol];
                    head.push(u8::try_from(p.spi.len()).unwrap_or(0));
                    head.push(u8::try_from(p.transforms.len()).unwrap_or(0));
                    head.extend_from_slice(&p.spi);
                    head.extend(t);
                    let l = len16(head.len());
                    head[2..4].copy_from_slice(&l);
                    v.extend(head);
                }
                v
            }
            Payload::Ke { group, data } => {
                let mut v = group.to_be_bytes().to_vec();
                v.extend_from_slice(&[0, 0]);
                v.extend_from_slice(data);
                v
            }
            Payload::IdI { kind, data } | Payload::IdR { kind, data } => {
                let mut v = vec![*kind, 0, 0, 0];
                v.extend_from_slice(data);
                v
            }
            Payload::Auth { method, data } => {
                let mut v = vec![*method, 0, 0, 0];
                v.extend_from_slice(data);
                v
            }
            Payload::Nonce(n) => n.clone(),
            Payload::Notify { protocol, spi, kind, data } => {
                let mut v = vec![*protocol, u8::try_from(spi.len()).unwrap_or(0)];
                v.extend_from_slice(&kind.to_be_bytes());
                v.extend_from_slice(spi);
                v.extend_from_slice(data);
                v
            }
            Payload::TsI(ts) | Payload::TsR(ts) => {
                let mut v = vec![u8::try_from(ts.len()).unwrap_or(0), 0, 0, 0];
                for s in ts {
                    v.extend_from_slice(&[TS_IPV4_ADDR_RANGE, s.protocol, 0, 16]);
                    v.extend_from_slice(&s.start_port.to_be_bytes());
                    v.extend_from_slice(&s.end_port.to_be_bytes());
                    v.extend_from_slice(&s.start.octets());
                    v.extend_from_slice(&s.end.octets());
                }
                v
            }
            Payload::Sk { body, .. } | Payload::Other { body, .. } => body.clone(),
        }
    }

    fn decode(kind: u8, b: &[u8], next: u8) -> Result<Payload> {
        let t = || WireError::Truncated("ikev2 payload");
        Ok(match kind {
            PT_SA => {
                let mut props = Vec::new();
                let mut at = 0;
                while at < b.len() && props.len() < MAX_ITEMS {
                    let last = *b.get(at).ok_or_else(t)?;
                    let plen = usize::from(be16(b, at + 2).ok_or_else(t)?);
                    let p = b.get(at..at + plen).ok_or_else(t)?;
                    if plen < 8 {
                        return Err(t());
                    }
                    let spi_len = usize::from(*p.get(6).ok_or_else(t)?);
                    let count = usize::from(*p.get(7).ok_or_else(t)?);
                    let spi = p.get(8..8 + spi_len).ok_or_else(t)?.to_vec();
                    let mut transforms = Vec::new();
                    let mut ta = 8 + spi_len;
                    for _ in 0..count.min(MAX_ITEMS) {
                        let tlen = usize::from(be16(p, ta + 2).ok_or_else(t)?);
                        let tr = p.get(ta..ta + tlen).ok_or_else(t)?;
                        if tlen < 8 {
                            return Err(t());
                        }
                        let mut key_len = None;
                        let mut aa = 8;
                        while aa + 4 <= tr.len() {
                            let ty = be16(tr, aa).ok_or_else(t)?;
                            if ty & 0x8000 != 0 {
                                if ty & 0x7fff == 14 {
                                    key_len = be16(tr, aa + 2);
                                }
                                aa += 4;
                            } else {
                                aa += 4 + usize::from(be16(tr, aa + 2).ok_or_else(t)?);
                            }
                        }
                        transforms.push(Transform { kind: *tr.get(4).ok_or_else(t)?, id: be16(tr, 6).ok_or_else(t)?, key_len });
                        ta += tlen;
                    }
                    props.push(Proposal { number: *p.get(4).ok_or_else(t)?, protocol: *p.get(5).ok_or_else(t)?, spi, transforms });
                    at += plen;
                    if last == 0 {
                        break;
                    }
                }
                Payload::Sa(props)
            }
            PT_KE => Payload::Ke { group: be16(b, 0).ok_or_else(t)?, data: b.get(4..).ok_or_else(t)?.to_vec() },
            PT_IDI | PT_IDR => {
                let k = *b.first().ok_or_else(t)?;
                let data = b.get(4..).ok_or_else(t)?.to_vec();
                if kind == PT_IDI { Payload::IdI { kind: k, data } } else { Payload::IdR { kind: k, data } }
            }
            PT_AUTH => Payload::Auth { method: *b.first().ok_or_else(t)?, data: b.get(4..).ok_or_else(t)?.to_vec() },
            PT_NONCE => Payload::Nonce(b.to_vec()),
            PT_NOTIFY => {
                let spi_len = usize::from(*b.get(1).ok_or_else(t)?);
                Payload::Notify {
                    protocol: *b.first().ok_or_else(t)?,
                    kind: be16(b, 2).ok_or_else(t)?,
                    spi: b.get(4..4 + spi_len).ok_or_else(t)?.to_vec(),
                    data: b.get(4 + spi_len..).ok_or_else(t)?.to_vec(),
                }
            }
            PT_TSI | PT_TSR => {
                let n = usize::from(*b.first().ok_or_else(t)?);
                let mut ts = Vec::new();
                let mut at = 4;
                for _ in 0..n.min(MAX_ITEMS) {
                    let len = usize::from(be16(b, at + 2).ok_or_else(t)?);
                    let s = b.get(at..at + len).ok_or_else(t)?;
                    if *s.first().ok_or_else(t)? == TS_IPV4_ADDR_RANGE && len == 16 {
                        let ip = |i: usize| -> Option<Ipv4Addr> { Some(Ipv4Addr::from(be32(s, i)?)) };
                        ts.push(TrafficSelector {
                            protocol: *s.get(1).ok_or_else(t)?,
                            start_port: be16(s, 4).ok_or_else(t)?,
                            end_port: be16(s, 6).ok_or_else(t)?,
                            start: ip(8).ok_or_else(t)?,
                            end: ip(12).ok_or_else(t)?,
                        });
                    }
                    at += len.max(4);
                }
                if kind == PT_TSI { Payload::TsI(ts) } else { Payload::TsR(ts) }
            }
            PT_SK => Payload::Sk { first: next, body: b.to_vec() },
            other => Payload::Other { kind: other, body: b.to_vec() },
        })
    }
}

/// The payloads as a chain (each generic header names the next payload's type).
fn chain(payloads: &[Payload]) -> Vec<u8> {
    let mut v = Vec::new();
    for (i, p) in payloads.iter().enumerate() {
        let next = payloads.get(i + 1).map_or(0, Payload::kind);
        let body = p.body();
        v.extend_from_slice(&[next, 0]);
        v.extend_from_slice(&len16(body.len() + 4));
        v.extend(body);
    }
    v
}

fn parse_chain(first: u8, b: &[u8]) -> Result<Vec<Payload>> {
    let t = || WireError::Truncated("ikev2 payload");
    let mut out = Vec::new();
    let mut kind = first;
    let mut at = 0;
    while kind != 0 && out.len() < MAX_ITEMS {
        let next = *b.get(at).ok_or_else(t)?;
        let len = usize::from(be16(b, at + 2).ok_or_else(t)?);
        if len < 4 {
            return Err(t());
        }
        let body = b.get(at + 4..at + len).ok_or_else(t)?;
        let p = Payload::decode(kind, body, next)?;
        out.push(p);
        // Nothing follows the SK payload in the clear.
        if kind == PT_SK {
            break;
        }
        kind = next;
        at += len;
    }
    Ok(out)
}

/// The keys of an IKE SA (RFC 7296 §2.14), for AES-128-CBC with HMAC-SHA1-96 and HMAC-SHA1
/// as the PRF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    pub sk_d: Vec<u8>,
    pub sk_ai: Vec<u8>,
    pub sk_ar: Vec<u8>,
    pub sk_ei: [u8; 16],
    pub sk_er: [u8; 16],
    pub sk_pi: Vec<u8>,
    pub sk_pr: Vec<u8>,
}

/// prf+ (RFC 7296 §2.13) with HMAC-SHA1: `len` bytes of keying material.
pub fn prf_plus(key: &[u8], seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 20);
    let mut prev: Vec<u8> = Vec::new();
    let mut n: u8 = 1;
    while out.len() < len {
        let mut input = prev.clone();
        input.extend_from_slice(seed);
        input.push(n);
        prev = hmac_sha1(key, &input).to_vec();
        out.extend_from_slice(&prev);
        n = n.wrapping_add(1);
    }
    out.truncate(len);
    out
}

impl Keys {
    /// SKEYSEED = prf(Ni | Nr, g^ir), then the seven keys from prf+(SKEYSEED, Ni | Nr | SPIi |
    /// SPIr). `shared` stands for g^ir (see the module notes).
    pub fn derive(shared: &[u8], ni: &[u8], nr: &[u8], spi_i: &[u8; 8], spi_r: &[u8; 8]) -> Keys {
        let skeyseed = hmac_sha1(&[ni, nr].concat(), shared);
        let seed = [ni, nr, &spi_i[..], &spi_r[..]].concat();
        let m = prf_plus(&skeyseed, &seed, 20 * 5 + 16 * 2);
        let part = |a: usize, b: usize| m.get(a..b).unwrap_or_default().to_vec();
        let key16 = |a: usize| -> [u8; 16] { m.get(a..a + 16).and_then(|s| s.try_into().ok()).unwrap_or([0; 16]) };
        Keys {
            sk_d: part(0, 20),
            sk_ai: part(20, 40),
            sk_ar: part(40, 60),
            sk_ei: key16(60),
            sk_er: key16(76),
            sk_pi: part(92, 112),
            sk_pr: part(112, 132),
        }
    }
}

/// The stand-in Diffie-Hellman result: a hash of both public KE values, in a fixed order.
pub fn standin_shared(ke_i: &[u8], ke_r: &[u8]) -> Vec<u8> {
    sha1(&[ke_i, ke_r].concat()).to_vec()
}

/// The AUTH value for a pre-shared key (RFC 7296 §2.15): prf(prf(key, "Key Pad for IKEv2"),
/// message | peer's nonce | prf(SK_p, ID body)).
pub fn psk_auth(psk: &[u8], message: &[u8], peer_nonce: &[u8], sk_p: &[u8], id_body: &[u8]) -> Vec<u8> {
    let pad = hmac_sha1(psk, b"Key Pad for IKEv2");
    let mac_id = hmac_sha1(sk_p, id_body);
    hmac_sha1(&pad, &[message, peer_nonce, &mac_id[..]].concat()).to_vec()
}

/// An IKEv2 message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub spi_i: [u8; 8],
    pub spi_r: [u8; 8],
    pub exchange: u8,
    pub flags: u8,
    pub message_id: u32,
    pub payloads: Vec<Payload>,
}

impl Message {
    fn header(&self, next: u8, total: usize) -> Vec<u8> {
        let mut v = self.spi_i.to_vec();
        v.extend_from_slice(&self.spi_r);
        v.extend_from_slice(&[next, VERSION, self.exchange, self.flags]);
        v.extend_from_slice(&self.message_id.to_be_bytes());
        v.extend_from_slice(&u32::try_from(total).unwrap_or(u32::MAX).to_be_bytes());
        v
    }

    /// The message in the clear (IKE_SA_INIT).
    pub fn encode(&self) -> Vec<u8> {
        let body = chain(&self.payloads);
        let mut v = self.header(self.payloads.first().map_or(0, Payload::kind), 28 + body.len());
        v.extend(body);
        v
    }

    /// The message with its payloads inside an SK payload, encrypted with `ek` and protected
    /// with `ak` (the sender's SK_e and SK_a). `iv` must be unpredictable to a real attacker;
    /// here the caller derives it.
    pub fn encode_sealed(&self, ek: &[u8; 16], ak: &[u8], iv: [u8; 16]) -> Vec<u8> {
        let mut plain = chain(&self.payloads);
        let pad = (BLOCK - (plain.len() + 1) % BLOCK) % BLOCK;
        plain.extend(std::iter::repeat_n(0u8, pad));
        plain.push(u8::try_from(pad).unwrap_or(0));
        let cipher = cbc_encrypt(&Aes128::new(ek), &iv, &plain);
        let sk_len = 4 + BLOCK + cipher.len() + ICV;
        let first = self.payloads.first().map_or(0, Payload::kind);
        let mut v = self.header(PT_SK, 28 + sk_len);
        v.extend_from_slice(&[first, 0]);
        v.extend_from_slice(&len16(sk_len));
        v.extend_from_slice(&iv);
        v.extend(cipher);
        let icv = hmac_sha1(ak, &v);
        v.extend_from_slice(icv.get(..ICV).unwrap_or_default());
        v
    }

    pub fn decode(b: &[u8]) -> Result<Message> {
        let t = || WireError::Truncated("ikev2");
        if b.len() > MAX_MESSAGE {
            return Err(WireError::Unsupported("ikev2", "message too long".into()));
        }
        if *b.get(17).ok_or_else(t)? >> 4 != 2 {
            return Err(WireError::Unsupported("ikev2", format!("version {}", b.get(17).copied().unwrap_or(0) >> 4)));
        }
        let total = usize::try_from(be32(b, 24).ok_or_else(t)?).map_err(|_| t())?;
        if total < 28 || b.len() < total {
            return Err(t());
        }
        let first = *b.get(16).ok_or_else(t)?;
        Ok(Message {
            spi_i: b.get(..8).and_then(|s| s.try_into().ok()).ok_or_else(t)?,
            spi_r: b.get(8..16).and_then(|s| s.try_into().ok()).ok_or_else(t)?,
            exchange: *b.get(18).ok_or_else(t)?,
            flags: *b.get(19).ok_or_else(t)?,
            message_id: be32(b, 20).ok_or_else(t)?,
            payloads: parse_chain(first, b.get(28..total).ok_or_else(t)?)?,
        })
    }

    /// The payloads inside the SK payload of the encoded message `raw`: integrity checked with
    /// `ak`, decrypted with `ek`.
    pub fn open(raw: &[u8], ek: &[u8; 16], ak: &[u8]) -> Result<Vec<Payload>> {
        let t = || WireError::Truncated("ikev2 sk");
        let m = Message::decode(raw)?;
        let Some(Payload::Sk { first, body }) = m.payloads.first() else {
            return Err(WireError::Unsupported("ikev2", "no encrypted payload".into()));
        };
        let total = usize::try_from(be32(raw, 24).ok_or_else(t)?).map_err(|_| t())?;
        let signed = raw.get(..total.checked_sub(ICV).ok_or_else(t)?).ok_or_else(t)?;
        let icv = raw.get(total - ICV..total).ok_or_else(t)?;
        if hmac_sha1(ak, signed).get(..ICV) != Some(icv) {
            return Err(WireError::Checksum("ikev2 integrity"));
        }
        let iv: [u8; 16] = body.get(..BLOCK).and_then(|s| s.try_into().ok()).ok_or_else(t)?;
        let cipher = body.get(BLOCK..body.len().checked_sub(ICV).ok_or_else(t)?).ok_or_else(t)?;
        if cipher.is_empty() || cipher.len() % BLOCK != 0 {
            return Err(t());
        }
        let plain = cbc_decrypt(&Aes128::new(ek), &iv, cipher);
        let pad = usize::from(*plain.last().ok_or_else(t)?);
        let inner = plain.get(..plain.len().checked_sub(pad + 1).ok_or_else(t)?).ok_or_else(t)?;
        parse_chain(*first, inner)
    }

    /// The first notification's type, if the message carries one.
    pub fn notify(payloads: &[Payload]) -> Option<u16> {
        payloads.iter().find_map(|p| match p {
            Payload::Notify { kind, .. } => Some(*kind),
            _ => None,
        })
    }
}

/// A one-line description for event lists.
pub fn summary(b: &[u8]) -> Option<String> {
    let m = Message::decode(b).ok()?;
    let names: Vec<&str> = m.payloads.iter().map(Payload::name).collect();
    let dir = if m.flags & FLAG_RESPONSE != 0 { "response" } else { "request" };
    Some(format!("IKEv2 {} {dir} #{} [{}]", exchange_name(m.exchange), m.message_id, names.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init() -> Message {
        Message {
            spi_i: [1, 2, 3, 4, 5, 6, 7, 8],
            spi_r: [0; 8],
            exchange: IKE_SA_INIT,
            flags: FLAG_INITIATOR,
            message_id: 0,
            payloads: vec![
                Payload::Sa(vec![
                    Proposal {
                        number: 1,
                        protocol: PROTO_IKE,
                        spi: Vec::new(),
                        transforms: vec![
                            Transform { kind: TT_ENCR, id: ENCR_AES_CBC, key_len: Some(128) },
                            Transform { kind: TT_PRF, id: PRF_HMAC_SHA1, key_len: None },
                            Transform { kind: TT_INTEG, id: AUTH_HMAC_SHA1_96, key_len: None },
                            Transform { kind: TT_DH, id: 14, key_len: None },
                        ],
                    },
                    Proposal { number: 2, protocol: PROTO_IKE, spi: Vec::new(), transforms: vec![Transform { kind: TT_DH, id: 19, key_len: None }] },
                ]),
                Payload::Ke { group: 14, data: vec![7; 256] },
                Payload::Nonce(vec![9; 32]),
            ],
        }
    }

    #[test]
    fn messages_round_trip_and_survive_truncation() {
        let m = init();
        let b = m.encode();
        assert_eq!(&b[16..20], &[PT_SA, 0x20, IKE_SA_INIT, FLAG_INITIATOR]);
        assert_eq!(be32(&b, 24), Some(b.len() as u32));
        assert_eq!(Message::decode(&b).unwrap(), m);
        assert_eq!(summary(&b).unwrap(), "IKEv2 IKE_SA_INIT request #0 [SA, KE, Nonce]");
        for n in 0..b.len() {
            let _ = Message::decode(&b[..n]);
        }
        // Wrong version.
        let mut v1 = b.clone();
        v1[17] = 0x10;
        assert!(Message::decode(&v1).is_err());
    }

    #[test]
    fn sealed_messages_open_only_with_the_right_keys() {
        let shared = standin_shared(&[1; 256], &[2; 256]);
        let k = Keys::derive(&shared, &[3; 32], &[4; 32], &[1; 8], &[2; 8]);
        assert_ne!(k.sk_ei, k.sk_er);
        let ts = TrafficSelector {
            protocol: 0,
            start_port: 0,
            end_port: 65_535,
            start: Ipv4Addr::new(192, 168, 1, 0),
            end: Ipv4Addr::new(192, 168, 1, 255),
        };
        let m = Message {
            spi_i: [1; 8],
            spi_r: [2; 8],
            exchange: IKE_AUTH,
            flags: FLAG_INITIATOR,
            message_id: 1,
            payloads: vec![
                Payload::IdI { kind: ID_IPV4_ADDR, data: vec![203, 0, 113, 1] },
                Payload::Auth { method: AUTH_SHARED_KEY, data: psk_auth(b"secret", b"msg1", &[4; 32], &k.sk_pi, &[1, 0, 0, 0, 203, 0, 113, 1]) },
                Payload::TsI(vec![ts]),
                Payload::TsR(vec![ts]),
                Payload::Notify { protocol: PROTO_IKE, spi: Vec::new(), kind: AUTHENTICATION_FAILED, data: Vec::new() },
            ],
        };
        let raw = m.encode_sealed(&k.sk_ei, &k.sk_ai, [5; 16]);
        assert_eq!(raw[16], PT_SK);
        assert_eq!(Message::open(&raw, &k.sk_ei, &k.sk_ai).unwrap(), m.payloads);
        assert_eq!(Message::notify(&m.payloads), Some(AUTHENTICATION_FAILED));
        // The other direction's keys don't open it; a flipped bit fails the integrity check.
        assert!(Message::open(&raw, &k.sk_er, &k.sk_ar).is_err());
        let mut bad = raw.clone();
        bad[40] ^= 1;
        assert_eq!(Message::open(&bad, &k.sk_ei, &k.sk_ai), Err(WireError::Checksum("ikev2 integrity")));
        for n in 0..raw.len() {
            let _ = Message::open(&raw[..n], &k.sk_ei, &k.sk_ai);
        }
        // Different keys give different AUTH values.
        assert_ne!(psk_auth(b"a", b"m", b"n", b"k", b"i"), psk_auth(b"b", b"m", b"n", b"k", b"i"));
    }

    #[test]
    fn prf_plus_chains_its_blocks() {
        // T1 = prf(K, S | 0x01), T2 = prf(K, T1 | S | 0x02).
        let out = prf_plus(b"key", b"seed", 40);
        let t1 = hmac_sha1(b"key", b"seed\x01");
        let t2 = hmac_sha1(b"key", &[&t1[..], b"seed\x02"].concat());
        assert_eq!(&out[..20], &t1);
        assert_eq!(&out[20..], &t2[..20]);
    }
}
