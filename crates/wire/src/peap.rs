//! The PEAP tunnel of WPA2-Enterprise logins, as a small self-contained TLS-like protocol.
//!
//! PEAP (EAP method 25) wraps an inner EAP login in a TLS tunnel between the host and the
//! authentication server, so the password never crosses the air or the wire readable. This
//! module is **not TLS**: no certificates, no cipher-suite negotiation, no real key exchange
//! group. It keeps TLS's shape so the exchange reads as it would and its secrecy is real:
//!
//! - Records as TLS has them: content type (22 handshake, 23 application data, 21 alert),
//!   version 3.3, a 16-bit length, the body.
//! - A handshake of three messages: the client hello (32-byte random, Diffie-Hellman public
//!   value), the server hello (random, public value, the server's name standing in for its
//!   certificate) and the server's Finished, then the client's Finished. Finished messages hold
//!   12 bytes of PRF over the transcript, as TLS 1.0/1.1 does (RFC 2246 §7.4.9).
//! - The key exchange is Diffie-Hellman in the multiplicative group modulo the Mersenne prime
//!   2^61 - 1 ([`DH_PRIME`]): a real exchange (the key isn't on the wire), but a toy size,
//!   far too small to resist a determined attacker. It is for watching the protocol, not for
//!   protecting anything.
//! - Keys come from the TLS 1.0 PRF style, with P_SHA1 only: the master secret, then a key
//!   block (client and server AES-128 keys and HMAC-SHA1 keys), and the EAP master session key
//!   (RFC 5216 §2.3's "client EAP encryption" label), whose first 32 bytes are the PMK.
//! - Application data is protected MAC-then-encrypt, TLS 1.1 style: HMAC-SHA1 over the
//!   sequence number, type, length and plaintext, then AES-128-CBC with an explicit IV and TLS
//!   padding.
//!
//! Inside the tunnel the inner method is EAP-GTC (RFC 3748 §5.6: the password in the clear,
//! which is why it needs the tunnel), after an inner identity exchange; the result travels as
//! a PEAPv0 Result TLV (EAP type 33, TLV type 3: 1 success, 2 failure).

use crate::aes::{Aes128, cbc_decrypt, cbc_encrypt};
use crate::sha1::{hmac_sha1, sha1};
use crate::{Result, WireError, be16};

/// PEAP's data flags: length included, more fragments, start; the low bits are the version (0).
pub const FLAG_LENGTH: u8 = 0x80;
pub const FLAG_MORE: u8 = 0x40;
pub const FLAG_START: u8 = 0x20;
/// TLS record content types.
pub const REC_ALERT: u8 = 21;
pub const REC_HANDSHAKE: u8 = 22;
pub const REC_APP_DATA: u8 = 23;
/// Handshake message types.
pub const HS_CLIENT_HELLO: u8 = 1;
pub const HS_SERVER_HELLO: u8 = 2;
pub const HS_FINISHED: u8 = 20;
/// Inner EAP methods: Generic Token Card, and PEAPv0's TLV extensions.
pub const EAP_GTC: u8 = 6;
pub const EAP_TLV: u8 = 33;
/// The Result TLV and its values.
pub const TLV_RESULT: u16 = 3;
pub const RESULT_SUCCESS: u16 = 1;
pub const RESULT_FAILURE: u16 = 2;
/// The toy Diffie-Hellman group: the prime 2^61 - 1 and a generator.
pub const DH_PRIME: u64 = (1 << 61) - 1;
pub const DH_GENERATOR: u64 = 37;
/// Most records one PEAP message carries, and the longest record body.
const MAX_RECORDS: usize = 16;
const MAX_RECORD: usize = 4096;

fn t() -> WireError {
    WireError::Truncated("peap")
}

fn mulmod(a: u64, b: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) % u128::from(DH_PRIME)) as u64
}

/// `base^exp mod DH_PRIME`.
pub fn powmod(base: u64, mut exp: u64) -> u64 {
    let mut b = base % DH_PRIME;
    let mut r = 1u64;
    while exp > 0 {
        if exp & 1 == 1 {
            r = mulmod(r, b);
        }
        b = mulmod(b, b);
        exp >>= 1;
    }
    r
}

/// A private exponent from 32 bytes of seed (never 0 or 1).
pub fn dh_secret(seed: &[u8]) -> u64 {
    let h = sha1(seed);
    let mut b = [0u8; 8];
    b.copy_from_slice(h.get(..8).unwrap_or(&[0; 8]));
    (u64::from_be_bytes(b) % (DH_PRIME - 3)) + 2
}

pub fn dh_public(secret: u64) -> u64 {
    powmod(DH_GENERATOR, secret)
}

/// The shared value from the peer's public value (0 for a value outside the group).
pub fn dh_shared(peer_public: u64, secret: u64) -> u64 {
    if (2..DH_PRIME).contains(&peer_public) { powmod(peer_public, secret) } else { 0 }
}

/// TLS 1.0's P_hash with HMAC-SHA1 (RFC 2246 §5): `len` bytes from `secret`, `label`, `seed`.
pub fn prf(secret: &[u8], label: &str, seed: &[u8], len: usize) -> Vec<u8> {
    let seed: Vec<u8> = label.as_bytes().iter().chain(seed).copied().collect();
    let mut out = Vec::with_capacity(len + 20);
    let mut a = hmac_sha1(secret, &seed).to_vec();
    while out.len() < len {
        let block = hmac_sha1(secret, &[a.as_slice(), &seed].concat());
        out.extend_from_slice(&block);
        a = hmac_sha1(secret, &a).to_vec();
    }
    out.truncate(len);
    out
}

/// One TLS-like record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    pub body: Vec<u8>,
}

impl Record {
    pub fn encode(&self) -> Vec<u8> {
        let body = self.body.get(..self.body.len().min(MAX_RECORD)).unwrap_or_default();
        let mut v = vec![self.kind, 3, 3];
        v.extend_from_slice(&(body.len() as u16).to_be_bytes());
        v.extend_from_slice(body);
        v
    }

    /// Every record in `b`.
    pub fn decode_all(b: &[u8]) -> Result<Vec<Record>> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < b.len() {
            if out.len() >= MAX_RECORDS {
                return Err(WireError::Unsupported("peap", "too many records".into()));
            }
            let kind = *b.get(at).ok_or_else(t)?;
            if !matches!(kind, REC_ALERT | REC_HANDSHAKE | REC_APP_DATA) {
                return Err(WireError::Unsupported("peap", format!("record type {kind}")));
            }
            let len = usize::from(be16(b, at + 3).ok_or_else(t)?);
            let body = b.get(at + 5..at + 5 + len).ok_or_else(t)?.to_vec();
            out.push(Record { kind, body });
            at += 5 + len;
        }
        Ok(out)
    }
}

/// One handshake message: type, 24-bit length, body.
pub fn handshake(kind: u8, body: &[u8]) -> Vec<u8> {
    let len = body.len().min(0xff_ffff);
    let mut v = vec![kind, (len >> 16) as u8, (len >> 8) as u8, len as u8];
    v.extend_from_slice(body.get(..len).unwrap_or_default());
    v
}

/// The handshake messages in a handshake record's body: (type, body).
pub fn handshakes(b: &[u8]) -> Result<Vec<(u8, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() && out.len() < MAX_RECORDS {
        let kind = *b.get(at).ok_or_else(t)?;
        let len = (usize::from(*b.get(at + 1).ok_or_else(t)?) << 16) | usize::from(be16(b, at + 2).ok_or_else(t)?);
        out.push((kind, b.get(at + 4..at + 4 + len).ok_or_else(t)?.to_vec()));
        at += 4 + len;
    }
    Ok(out)
}

/// A hello: the 32-byte random, the Diffie-Hellman public value, and (server) its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub random: [u8; 32],
    pub public: u64,
    pub name: String,
}

impl Hello {
    pub fn encode(&self) -> Vec<u8> {
        let name = self.name.as_bytes().get(..self.name.len().min(64)).unwrap_or_default();
        let mut v = self.random.to_vec();
        v.extend_from_slice(&self.public.to_be_bytes());
        v.push(name.len() as u8);
        v.extend_from_slice(name);
        v
    }

    pub fn decode(b: &[u8]) -> Result<Hello> {
        let random: [u8; 32] = b.get(..32).and_then(|r| r.try_into().ok()).ok_or_else(t)?;
        let public = u64::from_be_bytes(b.get(32..40).and_then(|r| r.try_into().ok()).ok_or_else(t)?);
        let n = usize::from(*b.get(40).ok_or_else(t)?);
        let name = String::from_utf8_lossy(b.get(41..41 + n).ok_or_else(t)?).into_owned();
        Ok(Hello { random, public, name })
    }
}

/// The keys of one tunnel, both directions, and its record sequence numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    pub master: Vec<u8>,
    client_key: [u8; 16],
    server_key: [u8; 16],
    client_mac: [u8; 20],
    server_mac: [u8; 20],
    pub client_seq: u64,
    pub server_seq: u64,
}

impl Keys {
    /// From the shared Diffie-Hellman value and both randoms.
    pub fn derive(shared: u64, client_random: &[u8; 32], server_random: &[u8; 32]) -> Keys {
        let randoms = [&client_random[..], &server_random[..]].concat();
        let master = prf(&shared.to_be_bytes(), "master secret", &randoms, 48);
        let block = prf(&master, "key expansion", &[&server_random[..], &client_random[..]].concat(), 72);
        let take = |a: usize| -> [u8; 20] {
            let mut k = [0u8; 20];
            k.copy_from_slice(block.get(a..a + 20).unwrap_or(&[0; 20]));
            k
        };
        let key = |a: usize| -> [u8; 16] {
            let mut k = [0u8; 16];
            k.copy_from_slice(block.get(a..a + 16).unwrap_or(&[0; 16]));
            k
        };
        Keys { client_mac: take(0), server_mac: take(20), client_key: key(40), server_key: key(56), master, client_seq: 0, server_seq: 0 }
    }

    /// The 12-byte verify data of a Finished message over the transcript so far.
    pub fn finished(&self, client: bool, transcript: &[u8]) -> Vec<u8> {
        prf(&self.master, if client { "client finished" } else { "server finished" }, &sha1(transcript), 12)
    }

    /// The EAP master session key (64 bytes); its first 32 are the WPA2 pairwise master key.
    pub fn msk(&self, client_random: &[u8; 32], server_random: &[u8; 32]) -> Vec<u8> {
        prf(&self.master, "client EAP encryption", &[&client_random[..], &server_random[..]].concat(), 64)
    }

    fn mac(key: &[u8], seq: u64, plain: &[u8]) -> [u8; 20] {
        let mut h = seq.to_be_bytes().to_vec();
        h.push(REC_APP_DATA);
        h.extend_from_slice(&[3, 3]);
        h.extend_from_slice(&(plain.len() as u16).to_be_bytes());
        h.extend_from_slice(plain);
        hmac_sha1(key, &h)
    }

    /// An application-data record from one side (`client`), with this explicit IV.
    pub fn seal(&mut self, client: bool, plain: &[u8], iv: [u8; 16]) -> Record {
        let plain = plain.get(..plain.len().min(MAX_RECORD - 64)).unwrap_or_default();
        let (key, mac, seq) =
            if client { (self.client_key, self.client_mac, &mut self.client_seq) } else { (self.server_key, self.server_mac, &mut self.server_seq) };
        let tag = Keys::mac(&mac, *seq, plain);
        *seq = seq.saturating_add(1);
        let mut data = plain.to_vec();
        data.extend_from_slice(&tag);
        // TLS padding: n+1 bytes of value n, to a whole block.
        let pad = 15 - (data.len() % 16);
        data.extend(std::iter::repeat_n(pad as u8, pad + 1));
        let mut body = iv.to_vec();
        body.extend(cbc_encrypt(&Aes128::new(&key), &iv, &data));
        Record { kind: REC_APP_DATA, body }
    }

    /// The plaintext of an application-data record from one side (`client`), checked.
    pub fn open(&mut self, client: bool, r: &Record) -> Result<Vec<u8>> {
        let bad = |why: &str| WireError::Unsupported("peap", why.to_string());
        if r.kind != REC_APP_DATA {
            return Err(bad("not application data"));
        }
        let iv: [u8; 16] = r.body.get(..16).and_then(|x| x.try_into().ok()).ok_or_else(t)?;
        let ct = r.body.get(16..).unwrap_or_default();
        if ct.is_empty() || !ct.len().is_multiple_of(16) {
            return Err(bad("record not a whole number of blocks"));
        }
        let (key, mac, seq) =
            if client { (self.client_key, self.client_mac, &mut self.client_seq) } else { (self.server_key, self.server_mac, &mut self.server_seq) };
        let data = cbc_decrypt(&Aes128::new(&key), &iv, ct);
        let pad = usize::from(*data.last().ok_or_else(t)?);
        let end = data.len().checked_sub(pad + 1 + 20).ok_or_else(|| bad("bad padding"))?;
        if data.get(end + 20..data.len().saturating_sub(1)).is_none_or(|p| p.iter().any(|&x| usize::from(x) != pad)) {
            return Err(bad("bad padding"));
        }
        let plain = data.get(..end).ok_or_else(t)?;
        let tag = data.get(end..end + 20).ok_or_else(t)?;
        if Keys::mac(&mac, *seq, plain) != tag {
            return Err(bad("record MAC check failed"));
        }
        *seq = seq.saturating_add(1);
        Ok(plain.to_vec())
    }
}

/// PEAP's data: flags, then the records (the length field is left out: messages are never
/// fragmented here).
pub fn peap_data(flags: u8, records: &[Record]) -> Vec<u8> {
    let mut v = vec![flags & !(FLAG_LENGTH | FLAG_MORE)];
    for r in records {
        v.extend(r.encode());
    }
    v
}

/// The flags and the records of PEAP's data (a length field, when flagged, is skipped).
pub fn parse_peap(data: &[u8]) -> Result<(u8, Vec<Record>)> {
    let flags = *data.first().ok_or_else(t)?;
    if flags & FLAG_MORE != 0 {
        return Err(WireError::Unsupported("peap", "fragmented messages".into()));
    }
    let start = if flags & FLAG_LENGTH != 0 { 5 } else { 1 };
    let rest = data.get(start..).ok_or_else(t)?;
    Ok((flags, Record::decode_all(rest)?))
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
    for _ in 0..MAX_RECORDS {
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
    match parse_peap(data) {
        Ok((flags, records)) if records.is_empty() => {
            if flags & FLAG_START != 0 {
                "start".into()
            } else {
                "acknowledgement".into()
            }
        }
        Ok((_, records)) => {
            let parts: Vec<String> = records
                .iter()
                .map(|r| match r.kind {
                    REC_HANDSHAKE => {
                        let names: Vec<&str> = handshakes(&r.body)
                            .unwrap_or_default()
                            .iter()
                            .map(|(k, _)| match *k {
                                HS_CLIENT_HELLO => "client hello",
                                HS_SERVER_HELLO => "server hello",
                                HS_FINISHED => "finished",
                                _ => "handshake message",
                            })
                            .collect();
                        format!("TLS handshake ({})", names.join(", "))
                    }
                    REC_APP_DATA => format!("TLS application data, {} encrypted bytes", r.body.len()),
                    _ => "TLS alert".into(),
                })
                .collect();
            parts.join(" + ")
        }
        Err(e) => format!("unreadable ({e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffie_hellman_agrees_and_the_prf_is_stable() {
        let (a, b) = (dh_secret(b"alice"), dh_secret(b"bob"));
        let (pa, pb) = (dh_public(a), dh_public(b));
        assert_ne!(pa, pb);
        assert_eq!(dh_shared(pb, a), dh_shared(pa, b));
        assert_ne!(dh_shared(pb, a), 0);
        assert_eq!(dh_shared(0, a), 0);
        assert_eq!(dh_shared(DH_PRIME, a), 0);
        assert_eq!(powmod(2, 61), 1, "2^61 = 1 mod 2^61 - 1");
        assert_eq!(powmod(DH_GENERATOR, DH_PRIME - 1), 1, "Fermat");
        let p = prf(b"secret", "label", b"seed", 45);
        assert_eq!(p.len(), 45);
        assert_eq!(prf(b"secret", "label", b"seed", 20), p[..20]);
        assert_ne!(prf(b"secret", "other", b"seed", 20), p[..20]);
    }

    #[test]
    fn records_seal_and_open_and_refuse_tampering_and_replays() {
        let (cr, sr) = ([1u8; 32], [2u8; 32]);
        let mut client = Keys::derive(12345, &cr, &sr);
        let mut server = client.clone();
        assert_eq!(client.finished(true, b"x").len(), 12);
        assert_ne!(client.finished(true, b"x"), client.finished(false, b"x"));
        assert_eq!(client.msk(&cr, &sr).len(), 64);
        for (n, msg) in [&b""[..], b"ana", b"a sixteen bytes!", &[7u8; 300]].into_iter().enumerate() {
            let r = client.seal(true, msg, [n as u8; 16]);
            assert!(!r.body.windows(msg.len().max(1)).any(|w| w == msg) || msg.is_empty());
            let back = Record::decode_all(&r.encode()).unwrap();
            assert_eq!(server.open(true, &back[0]).unwrap(), msg);
        }
        // A changed byte, a record played again, the other direction's keys.
        let r = client.seal(true, b"ana12345", [9; 16]);
        let mut bad = r.clone();
        let n = bad.body.len() - 20;
        bad.body[n] ^= 1;
        assert!(server.clone().open(true, &bad).is_err());
        assert!(server.clone().open(false, &r).is_err());
        assert_eq!(server.open(true, &r).unwrap(), b"ana12345");
        assert!(server.open(true, &r).is_err(), "replay: the sequence number moved on");
        // Other keys.
        let mut other = Keys::derive(54321, &cr, &sr);
        assert!(other.open(true, &client.seal(true, b"x", [0; 16])).is_err());
    }

    #[test]
    fn peap_data_round_trips_and_every_truncation_is_refused_not_fatal() {
        let hello = Hello { random: [5; 32], public: 77, name: "RADIUS1".into() };
        assert_eq!(Hello::decode(&hello.encode()).unwrap(), hello);
        let mut keys = Keys::derive(99, &[0; 32], &[1; 32]);
        let records = vec![
            Record { kind: REC_HANDSHAKE, body: [handshake(HS_SERVER_HELLO, &hello.encode()), handshake(HS_FINISHED, &[3; 12])].concat() },
            keys.seal(false, b"inner", [4; 16]),
        ];
        let data = peap_data(0, &records);
        let (flags, back) = parse_peap(&data).unwrap();
        assert_eq!((flags, back.clone()), (0, records));
        let hs = handshakes(&back[0].body).unwrap();
        assert_eq!(hs.iter().map(|h| h.0).collect::<Vec<_>>(), vec![HS_SERVER_HELLO, HS_FINISHED]);
        assert_eq!(describe(&data), "TLS handshake (server hello, finished) + TLS application data, 48 encrypted bytes");
        assert_eq!(describe(&[FLAG_START]), "start");
        assert_eq!(describe(&[0]), "acknowledgement");
        assert_eq!(tlv_result(&result_tlv(true)), Some(true));
        assert_eq!(tlv_result(&result_tlv(false)), Some(false));
        assert_eq!(tlv_result(&[0, 1]), None);
        for n in 0..data.len() {
            let _ = parse_peap(&data[..n]);
            let _ = describe(&data[..n]);
            let _ = handshakes(data.get(1..n).unwrap_or_default());
        }
        for n in 0..hello.encode().len() {
            assert!(Hello::decode(&hello.encode()[..n]).is_err());
        }
        let sealed = keys.seal(true, b"x", [0; 16]);
        for n in 0..sealed.body.len() {
            let _ = keys.clone().open(true, &Record { kind: REC_APP_DATA, body: sealed.body[..n].to_vec() });
        }
        assert!(parse_peap(&[FLAG_MORE]).is_err());
        assert!(Record::decode_all(&[99, 3, 3, 0, 0]).is_err());
    }
}
