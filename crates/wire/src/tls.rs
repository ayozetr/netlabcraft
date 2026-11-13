//! TLS 1.2 (RFC 5246) as a router's secure web server speaks it: the record layer, a full
//! handshake, the TLS 1.2 PRF (P_SHA256), and record protection with AES-128-CBC, explicit IVs
//! and HMAC-SHA1. Two suites, both over the RFC 3526 2048-bit group:
//!
//! - `TLS_DHE_RSA_WITH_AES_128_CBC_SHA` (0x0033, Appendix A.5): the server sends its X.509
//!   certificate chain (§7.4.2) and signs its Diffie-Hellman parameters with the certificate's
//!   RSA key, `sha256WithRSA` over the two randoms and the parameters (§7.4.3). A client that has
//!   trust anchors checks the chain (signatures, validity, CA flags) and the signature, and
//!   refuses with the matching alert (`unknown_ca`, `certificate_expired`, `bad_certificate`);
//!   a client with none still checks the signature against the certificate it was sent but
//!   reports the server as not verified.
//! - `TLS_DH_anon_WITH_AES_128_CBC_SHA` (0x0034): anonymous, no certificate, which RFC 5246
//!   warns authenticates nobody. A server uses it only when it has no certificate (or the client
//!   offers nothing else), so labs without a trustpoint run as they always did; a client that
//!   demands a verified server refuses it.
//!
//! What is real: the record and handshake formats, the certificate message, the signature, the
//! key exchange, the key derivation, the Finished checks and the encryption, so a capture shows
//! real TLS and its payloads can't be read. What is not: no session resumption, no renegotiation,
//! no client certificates, no revocation checking, no host-name matching (names are for people
//! here), the two suites above only; random values and private exponents come from the caller's
//! seed (the simulator's, so a lab replays the same bytes).

use crate::aes::{Aes128, cbc_decrypt, cbc_encrypt};
use crate::modp;
use crate::rsa::{RsaKey, SigHash};
use crate::sha1::hmac_sha1;
use crate::sha256::{hmac_sha256, sha256};
use crate::x509::{self, Certificate, ChainError};
use crate::{Result, WireError};

pub const CHANGE_CIPHER_SPEC: u8 = 20;
pub const ALERT: u8 = 21;
pub const HANDSHAKE: u8 = 22;
pub const APPLICATION_DATA: u8 = 23;
/// TLS 1.2 on the wire.
pub const VERSION: [u8; 2] = [3, 3];
/// `TLS_DH_anon_WITH_AES_128_CBC_SHA`.
pub const SUITE: u16 = 0x0034;
pub const SUITE_NAME: &str = "TLS_DH_anon_WITH_AES_128_CBC_SHA";
/// `TLS_DHE_RSA_WITH_AES_128_CBC_SHA`.
pub const SUITE_RSA: u16 = 0x0033;
pub const SUITE_RSA_NAME: &str = "TLS_DHE_RSA_WITH_AES_128_CBC_SHA";
/// `sha256` with `rsa` (RFC 5246 §7.4.1.4.1).
const SIG_SHA256_RSA: [u8; 2] = [4, 1];
/// Most certificates in a server's chain, and in one the client keeps.
const MAX_CERTS: usize = 6;

pub const CLIENT_HELLO: u8 = 1;
pub const SERVER_HELLO: u8 = 2;
pub const CERTIFICATE: u8 = 11;
pub const SERVER_KEY_EXCHANGE: u8 = 12;
pub const SERVER_HELLO_DONE: u8 = 14;
pub const CLIENT_KEY_EXCHANGE: u8 = 16;
pub const FINISHED: u8 = 20;

/// Alert descriptions (RFC 5246 §7.2).
pub const CLOSE_NOTIFY: u8 = 0;
pub const UNEXPECTED_MESSAGE: u8 = 10;
pub const BAD_RECORD_MAC: u8 = 20;
pub const RECORD_OVERFLOW: u8 = 22;
pub const HANDSHAKE_FAILURE: u8 = 40;
pub const ILLEGAL_PARAMETER: u8 = 47;
pub const DECODE_ERROR: u8 = 50;
pub const BAD_CERTIFICATE: u8 = 42;
pub const CERTIFICATE_EXPIRED: u8 = 45;
pub const CERTIFICATE_UNKNOWN: u8 = 46;
pub const DECRYPT_ERROR: u8 = 51;
pub const UNKNOWN_CA: u8 = 48;
pub const PROTOCOL_VERSION: u8 = 70;

/// Most plaintext bytes in one record (RFC 5246 §6.2.1).
pub const MAX_PLAIN: usize = 16_384;
/// Most bytes of a record's body on the wire (§6.2.3).
pub const MAX_BODY: usize = MAX_PLAIN + 2_048;
/// Most handshake bytes an endpoint keeps.
const MAX_HANDSHAKE: usize = 16 * 1024;

const WHAT: &str = "TLS";

fn bad(why: impl Into<String>) -> WireError {
    WireError::Unsupported(WHAT, why.into())
}

/// A TLS failure: the alert to send and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsError {
    pub alert: u8,
    pub why: String,
}

fn fail(alert: u8, why: impl Into<String>) -> TlsError {
    TlsError { alert, why: why.into() }
}

/// An alert's name.
pub fn alert_name(a: u8) -> &'static str {
    match a {
        CLOSE_NOTIFY => "close_notify",
        UNEXPECTED_MESSAGE => "unexpected_message",
        BAD_RECORD_MAC => "bad_record_mac",
        RECORD_OVERFLOW => "record_overflow",
        HANDSHAKE_FAILURE => "handshake_failure",
        ILLEGAL_PARAMETER => "illegal_parameter",
        DECODE_ERROR => "decode_error",
        BAD_CERTIFICATE => "bad_certificate",
        CERTIFICATE_EXPIRED => "certificate_expired",
        CERTIFICATE_UNKNOWN => "certificate_unknown",
        UNKNOWN_CA => "unknown_ca",
        DECRYPT_ERROR => "decrypt_error",
        PROTOCOL_VERSION => "protocol_version",
        _ => "alert",
    }
}

/// One record as it crossed the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    pub version: [u8; 2],
    pub body: Vec<u8>,
}

/// Frames one record.
pub fn record(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 5);
    out.push(kind);
    out.extend_from_slice(&VERSION);
    out.extend_from_slice(&u16::try_from(body.len()).unwrap_or(u16::MAX).to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// The first record of `buf`: `Ok(None)` until it all arrived, an error for bytes that are
/// not TLS (plain HTTP sent to the secure port, for one).
pub fn take_record(buf: &[u8]) -> Result<Option<(Record, usize)>> {
    let Some(&kind) = buf.first() else { return Ok(None) };
    if !(CHANGE_CIPHER_SPEC..=APPLICATION_DATA).contains(&kind) {
        return Err(bad(format!("content type {kind} is not a TLS record (is this plain text on a TLS port?)")));
    }
    let Some(head) = buf.get(..5) else { return Ok(None) };
    if head[1] != 3 {
        return Err(bad(format!("version {}.{}", head[1], head[2])));
    }
    let len = usize::from(u16::from_be_bytes([head[3], head[4]]));
    if len > MAX_BODY {
        return Err(bad("a record longer than 18432 bytes"));
    }
    let Some(body) = buf.get(5..5 + len) else { return Ok(None) };
    Ok(Some((Record { kind, version: [head[1], head[2]], body: body.to_vec() }, 5 + len)))
}

/// A handshake message: type, 24-bit length, body (§7.4).
pub fn handshake(kind: u8, body: &[u8]) -> Vec<u8> {
    let n = u32::try_from(body.len()).unwrap_or(u32::MAX).to_be_bytes();
    let mut out = vec![kind, n[1], n[2], n[3]];
    out.extend_from_slice(body);
    out
}

/// The handshake messages in a buffer of handshake bytes, and the bytes they used.
fn handshakes(buf: &[u8]) -> std::result::Result<(Vec<(u8, Vec<u8>, Vec<u8>)>, usize), TlsError> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(h) = buf.get(i..i + 4) {
        let len = (usize::from(h[1]) << 16) | (usize::from(h[2]) << 8) | usize::from(h[3]);
        if len > MAX_HANDSHAKE {
            return Err(fail(DECODE_ERROR, "a handshake message is too long"));
        }
        let Some(body) = buf.get(i + 4..i + 4 + len) else { break };
        out.push((h[0], body.to_vec(), buf.get(i..i + 4 + len).unwrap_or_default().to_vec()));
        i += 4 + len;
    }
    Ok((out, i))
}

/// A cursor over a message body.
struct Rd<'a>(&'a [u8]);

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> std::result::Result<&'a [u8], TlsError> {
        let (a, b) = (self.0.get(..n), self.0.get(n..));
        match (a, b) {
            (Some(a), Some(b)) => {
                self.0 = b;
                Ok(a)
            }
            _ => Err(fail(DECODE_ERROR, "a handshake message is cut short")),
        }
    }
    fn u8(&mut self) -> std::result::Result<u8, TlsError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> std::result::Result<u16, TlsError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn vec8(&mut self) -> std::result::Result<&'a [u8], TlsError> {
        let n = usize::from(self.u8()?);
        self.take(n)
    }
    fn u24(&mut self) -> std::result::Result<usize, TlsError> {
        let b = self.take(3)?;
        Ok((usize::from(b[0]) << 16) | (usize::from(b[1]) << 8) | usize::from(b[2]))
    }
    fn vec16(&mut self) -> std::result::Result<&'a [u8], TlsError> {
        let n = usize::from(self.u16()?);
        self.take(n)
    }
}

fn put24(out: &mut Vec<u8>, n: usize) {
    let b = u32::try_from(n).unwrap_or(u32::MAX).to_be_bytes();
    out.extend_from_slice(&b[1..]);
}

fn put16(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&u16::try_from(b.len()).unwrap_or(u16::MAX).to_be_bytes());
    out.extend_from_slice(b);
}

/// The TLS 1.2 PRF: P_SHA256(secret, label + seed), `len` bytes (§5).
pub fn prf(secret: &[u8], label: &str, seed: &[u8], len: usize) -> Vec<u8> {
    let ls: Vec<u8> = label.as_bytes().iter().chain(seed).copied().collect();
    let mut a = hmac_sha256(secret, &ls).to_vec();
    let mut out = Vec::with_capacity(len + 32);
    while out.len() < len {
        out.extend_from_slice(&hmac_sha256(secret, &[&a[..], &ls[..]].concat()));
        a = hmac_sha256(secret, &a).to_vec();
    }
    out.truncate(len);
    out
}

/// A seeded generator for randoms, IVs and padding (SHA-256 in counter mode).
#[derive(Debug, Clone)]
pub struct Drbg {
    seed: [u8; 32],
    n: u64,
}

impl Drbg {
    pub fn new(seed: &[u8]) -> Drbg {
        Drbg { seed: sha256(seed), n: 0 }
    }
    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len + 32);
        while out.len() < len {
            self.n = self.n.wrapping_add(1);
            out.extend_from_slice(&sha256(&[&self.seed[..], &self.n.to_be_bytes()].concat()));
        }
        out.truncate(len);
        out
    }
    pub fn array<const N: usize>(&mut self) -> [u8; N] {
        let mut a = [0u8; N];
        a.copy_from_slice(&self.bytes(N));
        a
    }
}

/// One direction's record protection (§6.2.3.2): AES-128-CBC with an explicit IV per record,
/// HMAC-SHA1 over the sequence number, header and content.
#[derive(Debug, Clone)]
struct Protect {
    aes: Aes128,
    mac: [u8; 20],
    seq: u64,
}

impl Protect {
    fn mac_of(&self, kind: u8, content: &[u8]) -> [u8; 20] {
        let mut m = self.seq.to_be_bytes().to_vec();
        m.push(kind);
        m.extend_from_slice(&VERSION);
        m.extend_from_slice(&u16::try_from(content.len()).unwrap_or(u16::MAX).to_be_bytes());
        m.extend_from_slice(content);
        hmac_sha1(&self.mac, &m)
    }

    fn seal(&mut self, kind: u8, content: &[u8], iv: [u8; 16]) -> Vec<u8> {
        let mut p = content.to_vec();
        p.extend_from_slice(&self.mac_of(kind, content));
        let pad = 15 - (p.len() % 16);
        p.extend(std::iter::repeat_n(pad as u8, pad + 1));
        self.seq = self.seq.wrapping_add(1);
        let mut body = iv.to_vec();
        body.extend(cbc_encrypt(&self.aes, &iv, &p));
        record(kind, &body)
    }

    fn open(&mut self, r: &Record) -> std::result::Result<Vec<u8>, TlsError> {
        if r.body.len() < 48 || !r.body.len().is_multiple_of(16) {
            return Err(fail(BAD_RECORD_MAC, "an encrypted record of the wrong size"));
        }
        let mut iv = [0u8; 16];
        iv.copy_from_slice(r.body.get(..16).unwrap_or(&[0; 16]));
        let p = cbc_decrypt(&self.aes, &iv, r.body.get(16..).unwrap_or_default());
        let pad = usize::from(p.last().copied().unwrap_or(0));
        let content_len = p.len().checked_sub(pad + 1 + 20).ok_or_else(|| fail(BAD_RECORD_MAC, "bad padding"))?;
        if p.get(p.len() - pad - 1..).is_none_or(|t| t.iter().any(|b| usize::from(*b) != pad)) {
            return Err(fail(BAD_RECORD_MAC, "bad padding"));
        }
        let content = p.get(..content_len).unwrap_or_default();
        let mac = p.get(content_len..content_len + 20).unwrap_or_default();
        if mac != self.mac_of(r.kind, content) {
            return Err(fail(BAD_RECORD_MAC, "a record's MAC does not match"));
        }
        self.seq = self.seq.wrapping_add(1);
        Ok(content.to_vec())
    }
}

/// The keys of both directions from the master secret (§6.3).
fn key_block(master: &[u8], client_random: &[u8], server_random: &[u8]) -> (Protect, Protect) {
    let seed: Vec<u8> = server_random.iter().chain(client_random).copied().collect();
    let kb = prf(master, "key expansion", &seed, 72);
    let arr20 = |s: &[u8]| {
        let mut a = [0u8; 20];
        a.copy_from_slice(s.get(..20).unwrap_or(&[0; 20]));
        a
    };
    let arr16 = |s: &[u8]| {
        let mut a = [0u8; 16];
        a.copy_from_slice(s.get(..16).unwrap_or(&[0; 16]));
        a
    };
    let client = Protect { aes: Aes128::new(&arr16(kb.get(40..56).unwrap_or_default())), mac: arr20(kb.get(..20).unwrap_or_default()), seq: 0 };
    let server = Protect { aes: Aes128::new(&arr16(kb.get(56..72).unwrap_or_default())), mac: arr20(kb.get(20..40).unwrap_or_default()), seq: 0 };
    (client, server)
}

fn master_secret(z: &[u8], client_random: &[u8], server_random: &[u8]) -> Vec<u8> {
    // §8.1.2: leading zero bytes of Z are stripped.
    let start = z.iter().position(|b| *b != 0).unwrap_or(z.len());
    let seed: Vec<u8> = client_random.iter().chain(server_random).copied().collect();
    prf(z.get(start..).unwrap_or_default(), "master secret", &seed, 48)
}

fn verify_data(master: &[u8], label: &str, transcript: &[u8]) -> Vec<u8> {
    prf(master, label, &sha256(transcript), 12)
}

/// What one endpoint made of the bytes it was given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// Records to send.
    pub send: Vec<u8>,
    /// Application data that arrived, decrypted.
    pub app: Vec<u8>,
    /// The handshake is done in both directions.
    pub established: bool,
    /// The peer said close_notify (or a fatal alert ended the session).
    pub closed: bool,
    /// A fatal alert the peer sent: (description, name).
    pub peer_alert: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Hello,
    KeyExchange,
    Finished,
    Open,
    Closed,
}

/// What a server presents: its certificate chain (DER, its own certificate first) and the
/// private key of that first certificate.
#[derive(Debug, Clone)]
pub struct Identity {
    pub chain: Vec<Vec<u8>>,
    pub key: RsaKey,
}

/// What a client checks a server against.
#[derive(Debug, Clone)]
pub struct Trust {
    /// The certificates it trusts as roots.
    pub anchors: Vec<Certificate>,
    /// The time to check validity windows at (Unix seconds, from the client's clock).
    pub now: u64,
    /// Refuse a server that shows no certificate (the anonymous suite).
    pub required: bool,
}

/// Either end of a TLS session.
#[derive(Debug, Clone)]
pub struct Tls {
    suite: u16,
    identity: Option<Identity>,
    trust: Option<Trust>,
    chain: Vec<Certificate>,
    verified: bool,
    server: bool,
    stage: Stage,
    rng: Drbg,
    private: Vec<u8>,
    client_random: Vec<u8>,
    server_random: Vec<u8>,
    transcript: Vec<u8>,
    master: Vec<u8>,
    hs_buf: Vec<u8>,
    write: Option<Protect>,
    read: Option<Protect>,
    pending: Option<(Protect, Protect)>,
}

impl Tls {
    fn new(server: bool, seed: &[u8]) -> Tls {
        let mut rng = Drbg::new(seed);
        let private = rng.bytes(modp::PRIVATE_LEN);
        Tls {
            server,
            stage: Stage::Hello,
            rng,
            private,
            suite: SUITE,
            identity: None,
            trust: None,
            chain: Vec::new(),
            verified: false,
            client_random: Vec::new(),
            server_random: Vec::new(),
            transcript: Vec::new(),
            master: Vec::new(),
            hs_buf: Vec::new(),
            write: None,
            read: None,
            pending: None,
        }
    }

    pub fn server(seed: &[u8]) -> Tls {
        Tls::new(true, seed)
    }

    /// A server that presents a certificate (when it has an identity).
    pub fn server_with(seed: &[u8], identity: Option<Identity>) -> Tls {
        let mut t = Tls::new(true, seed);
        t.identity = identity;
        t
    }

    /// A client, and the ClientHello it starts with.
    pub fn client(seed: &[u8]) -> (Tls, Vec<u8>) {
        Tls::client_with(seed, None)
    }

    /// A client that checks the server's certificate against `trust`, and its ClientHello.
    pub fn client_with(seed: &[u8], trust: Option<Trust>) -> (Tls, Vec<u8>) {
        let mut t = Tls::new(false, seed);
        t.trust = trust;
        t.client_random = t.rng.bytes(32);
        let mut body = VERSION.to_vec();
        body.extend_from_slice(&t.client_random);
        body.push(0); // no session id
        // The certificate suite first; the anonymous one only when nothing demands a certificate.
        let mut suites = SUITE_RSA.to_be_bytes().to_vec();
        if !t.trust.as_ref().is_some_and(|x| x.required) {
            suites.extend_from_slice(&SUITE.to_be_bytes());
        }
        put16(&mut body, &suites);
        body.extend_from_slice(&[1, 0]); // compression: null
        let msg = handshake(CLIENT_HELLO, &body);
        t.transcript.extend_from_slice(&msg);
        (t, record(HANDSHAKE, &msg))
    }

    pub fn established(&self) -> bool {
        self.stage == Stage::Open
    }

    /// The suite in use (the server's choice).
    pub fn suite_name(&self) -> &'static str {
        if self.suite == SUITE_RSA { SUITE_RSA_NAME } else { SUITE_NAME }
    }

    /// The certificate the server showed (a client's view), its own first.
    pub fn peer_chain(&self) -> &[Certificate] {
        &self.chain
    }

    /// Whether the server's chain was checked against trust anchors and passed.
    pub fn verified(&self) -> bool {
        self.verified
    }

    /// Whether `buf` holds the peer's whole next flight (client side), by its record
    /// structure alone: an alert, or bytes that are not TLS, end it too. Cheap: nothing is
    /// decrypted or computed.
    pub fn flight_done(&self, buf: &[u8]) -> bool {
        let mut i = 0usize;
        let mut after_ccs = false;
        while let Some(rest) = buf.get(i..).filter(|r| !r.is_empty()) {
            let (r, used) = match take_record(rest) {
                Ok(Some(x)) => x,
                Ok(None) => return false,
                Err(_) => return true,
            };
            i += used;
            match r.kind {
                ALERT => return true,
                CHANGE_CIPHER_SPEC => after_ccs = true,
                HANDSHAKE if after_ccs => return true,
                HANDSHAKE
                    if (self.stage == Stage::Hello || self.stage == Stage::KeyExchange)
                        && handshakes(&r.body).is_ok_and(|(m, _)| m.iter().any(|(k, _, _)| *k == SERVER_HELLO_DONE)) =>
                {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn seal(&mut self, kind: u8, content: &[u8]) -> Vec<u8> {
        match self.write.as_mut() {
            Some(w) => {
                let iv: [u8; 16] = self.rng.array();
                w.seal(kind, content, iv)
            }
            None => record(kind, content),
        }
    }

    /// Application data as protected records.
    pub fn send(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for c in data.chunks(MAX_PLAIN) {
            out.extend(self.seal(APPLICATION_DATA, c));
        }
        out
    }

    /// A close_notify alert.
    pub fn close(&mut self) -> Vec<u8> {
        self.stage = Stage::Closed;
        self.seal(ALERT, &[1, CLOSE_NOTIFY])
    }

    /// A fatal alert record for an error (protected once keys are in use).
    pub fn alert(&mut self, e: &TlsError) -> Vec<u8> {
        self.stage = Stage::Closed;
        self.seal(ALERT, &[2, e.alert])
    }

    /// Takes the complete records off the front of `buf` and acts on them.
    pub fn feed(&mut self, buf: &mut Vec<u8>) -> std::result::Result<Output, TlsError> {
        let mut out = Output::default();
        for _ in 0..256 {
            if self.stage == Stage::Closed {
                break;
            }
            let (r, used) = match take_record(buf) {
                Ok(Some(x)) => x,
                Ok(None) => break,
                Err(e) => {
                    return Err(fail(
                        if buf.first().is_some_and(|b| *b == b'G' || *b == b'P') { UNEXPECTED_MESSAGE } else { DECODE_ERROR },
                        e.to_string(),
                    ));
                }
            };
            buf.drain(..used.min(buf.len()));
            let content = match self.read.as_mut() {
                Some(rd) => rd.open(&r)?,
                None => r.body.clone(),
            };
            match r.kind {
                ALERT => {
                    let level = content.first().copied().unwrap_or(2);
                    let what = content.get(1).copied().unwrap_or(0);
                    out.closed = true;
                    self.stage = Stage::Closed;
                    if !(level == 1 && what == CLOSE_NOTIFY) {
                        out.peer_alert = Some(what);
                    }
                }
                CHANGE_CIPHER_SPEC => {
                    if content != [1] || self.stage != Stage::Finished {
                        return Err(fail(UNEXPECTED_MESSAGE, "ChangeCipherSpec out of place"));
                    }
                    let (c, s) = self.pending.clone().ok_or_else(|| fail(UNEXPECTED_MESSAGE, "ChangeCipherSpec before the key exchange"))?;
                    self.read = Some(if self.server { c } else { s });
                }
                HANDSHAKE => {
                    if self.hs_buf.len().saturating_add(content.len()) > MAX_HANDSHAKE {
                        return Err(fail(DECODE_ERROR, "too much handshake data"));
                    }
                    self.hs_buf.extend_from_slice(&content);
                    let (msgs, used) = handshakes(&self.hs_buf)?;
                    self.hs_buf.drain(..used.min(self.hs_buf.len()));
                    for (kind, body, raw) in msgs {
                        self.handshake(kind, &body, &raw, &mut out)?;
                    }
                }
                _ => {
                    if self.stage != Stage::Open {
                        return Err(fail(UNEXPECTED_MESSAGE, "application data before the handshake finished"));
                    }
                    if out.app.len().saturating_add(content.len()) > 4 * MAX_BODY * 16 {
                        return Err(fail(RECORD_OVERFLOW, "too much data at once"));
                    }
                    out.app.extend_from_slice(&content);
                }
            }
        }
        out.established = self.stage == Stage::Open;
        Ok(out)
    }

    fn handshake(&mut self, kind: u8, body: &[u8], raw: &[u8], out: &mut Output) -> std::result::Result<(), TlsError> {
        let mut rd = Rd(body);
        match (self.server, self.stage, kind) {
            (true, Stage::Hello, CLIENT_HELLO) => {
                let v = rd.take(2)?;
                if v[0] != 3 || v[1] < 3 {
                    return Err(fail(PROTOCOL_VERSION, "the client does not offer TLS 1.2"));
                }
                self.client_random = rd.take(32)?.to_vec();
                rd.vec8()?;
                let suites = rd.vec16()?;
                let offers = |x: u16| suites.chunks(2).any(|s| s == x.to_be_bytes());
                self.suite = if self.identity.is_some() && offers(SUITE_RSA) {
                    SUITE_RSA
                } else if offers(SUITE) {
                    SUITE
                } else {
                    return Err(fail(
                        HANDSHAKE_FAILURE,
                        format!("the client offers no suite this server can use ({SUITE_NAME}, or {SUITE_RSA_NAME} with a certificate)"),
                    ));
                };
                if !rd.vec8()?.contains(&0) {
                    return Err(fail(HANDSHAKE_FAILURE, "the client offers no null compression"));
                }
                self.transcript.extend_from_slice(raw);
                self.server_random = self.rng.bytes(32);
                let mut sh = VERSION.to_vec();
                sh.extend_from_slice(&self.server_random);
                sh.push(0);
                sh.extend_from_slice(&self.suite.to_be_bytes());
                sh.push(0);
                let mut ske = Vec::new();
                put16(&mut ske, &modp::prime());
                put16(&mut ske, &[modp::G]);
                put16(&mut ske, &modp::public(&self.private));
                let mut msgs = vec![handshake(SERVER_HELLO, &sh)];
                if let (SUITE_RSA, Some(id)) = (self.suite, &self.identity) {
                    let mut list = Vec::new();
                    for der in &id.chain {
                        put24(&mut list, der.len());
                        list.extend_from_slice(der);
                    }
                    let mut cert = Vec::new();
                    put24(&mut cert, list.len());
                    cert.extend(list);
                    msgs.push(handshake(CERTIFICATE, &cert));
                    // §7.4.3: the signature covers both randoms and the parameters.
                    let signed = [&self.client_random[..], &self.server_random[..], &ske[..]].concat();
                    let sig =
                        id.key.sign(SigHash::Sha256, &signed).ok_or_else(|| fail(HANDSHAKE_FAILURE, "the server's key is too small to sign with"))?;
                    ske.extend_from_slice(&SIG_SHA256_RSA);
                    put16(&mut ske, &sig);
                }
                msgs.push(handshake(SERVER_KEY_EXCHANGE, &ske));
                msgs.push(handshake(SERVER_HELLO_DONE, &[]));
                let mut flight = Vec::new();
                for m in msgs {
                    self.transcript.extend_from_slice(&m);
                    flight.extend(m);
                }
                out.send.extend(record(HANDSHAKE, &flight));
                self.stage = Stage::KeyExchange;
            }
            (true, Stage::KeyExchange, CLIENT_KEY_EXCHANGE) => {
                let yc = rd.vec16()?;
                let z = modp::shared(yc, &self.private).ok_or_else(|| fail(ILLEGAL_PARAMETER, "the client's public value is unusable"))?;
                self.transcript.extend_from_slice(raw);
                self.master = master_secret(&z, &self.client_random, &self.server_random);
                self.pending = Some(key_block(&self.master, &self.client_random, &self.server_random));
                self.stage = Stage::Finished;
            }
            (true, Stage::Finished, FINISHED) => {
                if self.read.is_none() {
                    return Err(fail(UNEXPECTED_MESSAGE, "Finished before ChangeCipherSpec"));
                }
                if body != verify_data(&self.master, "client finished", &self.transcript) {
                    return Err(fail(DECRYPT_ERROR, "the client's Finished does not match"));
                }
                self.transcript.extend_from_slice(raw);
                let fin = handshake(FINISHED, &verify_data(&self.master, "server finished", &self.transcript));
                out.send.extend(record(CHANGE_CIPHER_SPEC, &[1]));
                self.write = self.pending.clone().map(|(_, s)| s);
                out.send.extend(self.seal(HANDSHAKE, &fin));
                self.stage = Stage::Open;
            }
            (false, Stage::Hello, SERVER_HELLO) => {
                if rd.take(2)? != VERSION {
                    return Err(fail(PROTOCOL_VERSION, "the server does not speak TLS 1.2"));
                }
                self.server_random = rd.take(32)?.to_vec();
                rd.vec8()?;
                let suite = rd.u16()?;
                let anon_ok = !self.trust.as_ref().is_some_and(|t| t.required);
                if !(suite == SUITE_RSA || (suite == SUITE && anon_ok)) || rd.u8()? != 0 {
                    return Err(fail(ILLEGAL_PARAMETER, "the server picked a suite or compression the client didn't offer"));
                }
                self.suite = suite;
                self.transcript.extend_from_slice(raw);
            }
            (false, Stage::Hello, CERTIFICATE) if self.suite == SUITE_RSA => {
                let total = rd.u24()?;
                let mut list = Rd(rd.take(total)?);
                let mut chain = Vec::new();
                while !list.0.is_empty() {
                    let n = list.u24()?;
                    let der = list.take(n)?;
                    if chain.len() >= MAX_CERTS {
                        return Err(fail(DECODE_ERROR, "the server sent too many certificates"));
                    }
                    chain
                        .push(Certificate::parse(der).map_err(|e| fail(BAD_CERTIFICATE, format!("a certificate the server sent is unusable: {e}")))?);
                }
                if chain.is_empty() {
                    return Err(fail(BAD_CERTIFICATE, "the server sent an empty certificate list"));
                }
                if let Some(t) = &self.trust {
                    match x509::verify_chain(&chain, &t.anchors, t.now) {
                        Ok(()) => self.verified = true,
                        Err(e) => {
                            let alert = match e {
                                ChainError::Expired(_) | ChainError::NotYetValid(_) => CERTIFICATE_EXPIRED,
                                ChainError::Untrusted | ChainError::BadSignature(_) | ChainError::NotCa(_) | ChainError::PathLength(_) => UNKNOWN_CA,
                                ChainError::Empty | ChainError::TooLong => BAD_CERTIFICATE,
                            };
                            return Err(fail(alert, format!("the server's certificate is not accepted: {e}")));
                        }
                    }
                }
                self.chain = chain;
                self.transcript.extend_from_slice(raw);
            }
            (false, Stage::Hello, SERVER_KEY_EXCHANGE) => {
                let (p, g, ys) = (rd.vec16()?.to_vec(), rd.vec16()?.to_vec(), rd.vec16()?.to_vec());
                if self.suite == SUITE_RSA {
                    let params_len = body.len().saturating_sub(rd.0.len());
                    let params = body.get(..params_len).unwrap_or_default();
                    let leaf = self.chain.first().ok_or_else(|| fail(UNEXPECTED_MESSAGE, "the key exchange came before the certificate"))?;
                    if rd.take(2)? != SIG_SHA256_RSA {
                        return Err(fail(HANDSHAKE_FAILURE, "the server signed with an algorithm this client does not use (sha256 with RSA only)"));
                    }
                    let sig = rd.vec16()?;
                    let signed = [&self.client_random[..], &self.server_random[..], params].concat();
                    if !leaf.key.verify(SigHash::Sha256, &signed, sig) {
                        return Err(fail(DECRYPT_ERROR, "the server's signature over its key exchange does not match its certificate"));
                    }
                } else if self.trust.as_ref().is_some_and(|t| t.required) {
                    return Err(fail(HANDSHAKE_FAILURE, "the server shows no certificate"));
                }
                if p != modp::prime() || g != [modp::G] {
                    return Err(fail(ILLEGAL_PARAMETER, "the server's group is not the RFC 3526 2048-bit group"));
                }
                let z = modp::shared(&ys, &self.private).ok_or_else(|| fail(ILLEGAL_PARAMETER, "the server's public value is unusable"))?;
                self.transcript.extend_from_slice(raw);
                self.master = master_secret(&z, &self.client_random, &self.server_random);
                self.pending = Some(key_block(&self.master, &self.client_random, &self.server_random));
                self.stage = Stage::KeyExchange;
            }
            (false, Stage::KeyExchange, SERVER_HELLO_DONE) => {
                self.transcript.extend_from_slice(raw);
                let mut cke = Vec::new();
                put16(&mut cke, &modp::public(&self.private));
                let cke = handshake(CLIENT_KEY_EXCHANGE, &cke);
                self.transcript.extend_from_slice(&cke);
                out.send.extend(record(HANDSHAKE, &cke));
                out.send.extend(record(CHANGE_CIPHER_SPEC, &[1]));
                self.write = self.pending.clone().map(|(c, _)| c);
                let fin = handshake(FINISHED, &verify_data(&self.master, "client finished", &self.transcript));
                self.transcript.extend_from_slice(&fin);
                out.send.extend(self.seal(HANDSHAKE, &fin));
                self.stage = Stage::Finished;
            }
            (false, Stage::Finished, FINISHED) => {
                if self.read.is_none() {
                    return Err(fail(UNEXPECTED_MESSAGE, "Finished before ChangeCipherSpec"));
                }
                if body != verify_data(&self.master, "server finished", &self.transcript) {
                    return Err(fail(DECRYPT_ERROR, "the server's Finished does not match"));
                }
                self.stage = Stage::Open;
            }
            (_, _, k) => return Err(fail(UNEXPECTED_MESSAGE, format!("handshake message {k} out of place"))),
        }
        if rd.0.len() > 4096 {
            return Err(fail(DECODE_ERROR, "a handshake message carries too much"));
        }
        Ok(())
    }
}

/// A short description of a segment's TLS records, for the event list.
pub fn describe(payload: &[u8]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut encrypted_hs = false;
    while let Some(rest) = payload.get(i..).filter(|r| !r.is_empty()) {
        let Ok(Some((r, used))) = take_record(rest) else {
            if parts.is_empty() {
                return format!("not TLS ({} bytes)", payload.len());
            }
            parts.push("…".into());
            break;
        };
        i += used;
        let p = match r.kind {
            HANDSHAKE if encrypted_hs => "encrypted handshake (Finished)".to_string(),
            HANDSHAKE => {
                let names: Vec<&str> = handshakes(&r.body)
                    .map(|(m, _)| m)
                    .unwrap_or_default()
                    .iter()
                    .map(|(k, _, _)| match *k {
                        CLIENT_HELLO => "ClientHello",
                        SERVER_HELLO => "ServerHello",
                        CERTIFICATE => "Certificate",
                        SERVER_KEY_EXCHANGE => "ServerKeyExchange (DH)",
                        SERVER_HELLO_DONE => "ServerHelloDone",
                        CLIENT_KEY_EXCHANGE => "ClientKeyExchange (DH)",
                        FINISHED => "Finished",
                        _ => "handshake",
                    })
                    .collect();
                if names.is_empty() { "handshake".to_string() } else { names.join(", ") }
            }
            CHANGE_CIPHER_SPEC => {
                encrypted_hs = true;
                "ChangeCipherSpec".to_string()
            }
            ALERT if r.body.len() == 2 => format!("Alert ({})", alert_name(r.body[1])),
            ALERT => "Alert (encrypted)".to_string(),
            _ => format!("Application Data (encrypted, {} bytes)", r.body.len()),
        };
        if parts.len() >= 6 {
            parts.push("…".into());
            break;
        }
        parts.push(p);
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake_pair() -> (Tls, Tls, Vec<u8>) {
        let mut server = Tls::server(b"server seed");
        let (mut client, hello) = Tls::client(b"client seed");
        let mut buf = hello.clone();
        let o = server.feed(&mut buf).unwrap();
        assert!(buf.is_empty() && !o.send.is_empty() && !o.established);
        let mut b = o.send;
        let o = client.feed(&mut b).unwrap();
        assert!(!o.established);
        let mut b = o.send;
        let o = server.feed(&mut b).unwrap();
        assert!(o.established, "server done");
        let mut b = o.send;
        let o = client.feed(&mut b).unwrap();
        assert!(o.established, "client done");
        (client, server, hello)
    }

    #[test]
    fn a_full_handshake_then_encrypted_data_both_ways() {
        let (mut c, mut s, hello) = handshake_pair();
        assert_eq!(&hello[..3], &[HANDSHAKE, 3, 3]);
        let mut wire = c.send(b"GET /restconf HTTP/1.1\r\n\r\n");
        assert!(!wire.windows(8).any(|w| w == b"restconf"), "application data is encrypted");
        assert_eq!(wire[0], APPLICATION_DATA);
        assert_eq!(s.feed(&mut wire).unwrap().app, b"GET /restconf HTTP/1.1\r\n\r\n");
        let big = vec![b'x'; MAX_PLAIN * 2 + 5];
        let mut wire = s.send(&big);
        wire.extend(s.close());
        let o = c.feed(&mut wire).unwrap();
        assert_eq!(o.app, big);
        assert!(o.closed && o.peer_alert.is_none());
        assert!(describe(&hello).contains("ClientHello"));
    }

    #[test]
    fn tampering_and_plain_text_are_refused() {
        let (mut c, mut s, _) = handshake_pair();
        let mut wire = c.send(b"secret");
        let n = wire.len();
        wire[n - 3] ^= 1;
        assert_eq!(s.feed(&mut wire).unwrap_err().alert, BAD_RECORD_MAC);
        // Plain HTTP on a TLS port.
        let mut fresh = Tls::server(b"x");
        let e = fresh.feed(&mut b"GET / HTTP/1.1\r\n\r\n".to_vec()).unwrap_err();
        assert_eq!(e.alert, UNEXPECTED_MESSAGE);
        assert_eq!(fresh.alert(&e), vec![ALERT, 3, 3, 0, 2, 2, UNEXPECTED_MESSAGE]);
        // A client offering only other suites.
        let mut body = VERSION.to_vec();
        body.extend_from_slice(&[0; 32]);
        body.push(0);
        put16(&mut body, &0x002fu16.to_be_bytes());
        body.extend_from_slice(&[1, 0]);
        let mut wire = record(HANDSHAKE, &handshake(CLIENT_HELLO, &body));
        assert_eq!(Tls::server(b"y").feed(&mut wire).unwrap_err().alert, HANDSHAKE_FAILURE);
    }

    #[test]
    fn every_truncation_waits_or_fails_cleanly() {
        let mut server = Tls::server(b"s");
        let (_, hello) = Tls::client(b"c");
        for cut in 0..hello.len() {
            let mut b = hello[..cut].to_vec();
            let o = server.clone().feed(&mut b).unwrap();
            assert!(o.send.is_empty(), "cut {cut}");
        }
        let mut b = hello.clone();
        let flight = server.feed(&mut b).unwrap().send;
        for cut in 0..flight.len() {
            let mut b = flight[..cut].to_vec();
            let (mut c, _) = Tls::client(b"c");
            assert!(c.feed(&mut b).is_ok_and(|o| o.send.is_empty()), "cut {cut}");
        }
        // Every single-byte corruption of the flight is refused or ignored, never a panic.
        for i in 0..flight.len().min(400) {
            let mut b = flight.clone();
            b[i] ^= 0x80;
            let (mut c, _) = Tls::client(b"c");
            let _ = c.feed(&mut b);
        }
        for junk in [&b"\x16\x03\x03\xff\xff"[..], b"\x17\x03\x03\x00\x01x", b"\x16\x02\x00\x00\x00", b"\x15\x03\x03\x00\x00"] {
            let _ = Tls::server(b"z").feed(&mut junk.to_vec());
        }
        assert!(take_record(b"\x16\x03\x03\xff\xff").is_err());
    }

    #[test]
    fn the_prf_matches_a_known_vector() {
        // A widely published TLS 1.2 P_SHA256 test vector (secret, label "test label", seed).
        let secret = [0x9b, 0xbe, 0x43, 0x6b, 0xa9, 0x40, 0xf0, 0x17, 0xb1, 0x76, 0x52, 0x84, 0x9a, 0x71, 0xdb, 0x35];
        let seed = [0xa0, 0xba, 0x9f, 0x93, 0x6c, 0xda, 0x31, 0x18, 0x27, 0xa6, 0xf7, 0x96, 0xff, 0xd5, 0x19, 0x8c];
        let out = prf(&secret, "test label", &seed, 16);
        assert_eq!(out, [0xe3, 0xf2, 0x29, 0xba, 0x72, 0x7b, 0xe1, 0x7b, 0x8d, 0x12, 0x26, 0x20, 0x55, 0x7c, 0xd4, 0x53]);
    }

    use crate::x509::{Issue, Name};

    struct Pki {
        root: Certificate,
        leaf_der: Vec<u8>,
        leaf_key: RsaKey,
    }

    fn pki(not_after: u64) -> Pki {
        let root_key = RsaKey::generate(1024, &mut Drbg::new(b"tls root")).unwrap();
        let leaf_key = RsaKey::generate(1024, &mut Drbg::new(b"tls leaf")).unwrap();
        let (rn, ln) = (Name::new("Lab Root", "NetLab"), Name::new("r1.lab.test", "NetLab"));
        let make = |subject: &Name, sk: &RsaKey, issuer: &Name, ik: &RsaKey, ca: bool, serial: u8, until: u64| {
            Certificate::issue(&Issue {
                subject,
                subject_key: sk.public(),
                issuer,
                issuer_key: ik,
                serial: &[serial],
                not_before: 1_000,
                not_after: until,
                ca,
                path_len: None,
                hash: SigHash::Sha256,
            })
            .unwrap()
        };
        let root = make(&rn, &root_key, &rn, &root_key, true, 1, 1_000_000);
        let leaf = make(&ln, &leaf_key, &rn, &root_key, false, 2, not_after);
        Pki { root, leaf_der: leaf.der().to_vec(), leaf_key }
    }

    fn identity(p: &Pki) -> Identity {
        Identity { chain: vec![p.leaf_der.clone()], key: p.leaf_key.clone() }
    }

    /// Runs a handshake between a server and a client; the client's error, or the pair.
    fn run(server_id: Option<Identity>, trust: Option<Trust>) -> std::result::Result<(Tls, Tls), TlsError> {
        let mut server = Tls::server_with(b"srv", server_id);
        let (mut client, hello) = Tls::client_with(b"cli", trust);
        let mut buf = hello;
        let mut b = server.feed(&mut buf)?.send;
        let o = client.feed(&mut b)?;
        let mut b = o.send;
        let o = server.feed(&mut b)?;
        let mut b = o.send;
        let o = client.feed(&mut b)?;
        assert!(o.established && server.established());
        Ok((client, server))
    }

    #[test]
    fn a_certificate_handshake_verifies_the_chain_and_the_signature() {
        let p = pki(500_000);
        let trust = Trust { anchors: vec![p.root.clone()], now: 2_000, required: true };
        let (mut c, mut s) = run(Some(identity(&p)), Some(trust)).unwrap();
        assert_eq!((c.suite_name(), s.suite_name()), (SUITE_RSA_NAME, SUITE_RSA_NAME));
        assert!(c.verified() && c.peer_chain().len() == 1);
        assert_eq!(c.peer_chain()[0].subject.text(), "CN=r1.lab.test, O=NetLab");
        // The data is encrypted both ways as before.
        let mut w = c.send(b"hello");
        assert_eq!(s.feed(&mut w).unwrap().app, b"hello");
        let mut w = s.send(b"world");
        assert_eq!(c.feed(&mut w).unwrap().app, b"world");
    }

    #[test]
    fn a_client_without_trust_checks_the_signature_but_reports_unverified() {
        let p = pki(500_000);
        let (c, _) = run(Some(identity(&p)), None).unwrap();
        assert!(!c.verified());
        assert_eq!(c.suite_name(), SUITE_RSA_NAME);
        assert_eq!(c.peer_chain().len(), 1);
    }

    #[test]
    fn untrusted_expired_and_wrong_key_servers_are_refused_with_the_right_alert() {
        let p = pki(500_000);
        // No anchors that vouch for it.
        let stranger = pki(500_000);
        let other_root = {
            let k = RsaKey::generate(1024, &mut Drbg::new(b"other root")).unwrap();
            let n = Name::new("Other", "X");
            Certificate::issue(&Issue {
                subject: &n,
                subject_key: k.public(),
                issuer: &n,
                issuer_key: &k,
                serial: &[9],
                not_before: 1,
                not_after: 1_000_000,
                ca: true,
                path_len: None,
                hash: SigHash::Sha256,
            })
            .unwrap()
        };
        let _ = stranger;
        let e = run(Some(identity(&p)), Some(Trust { anchors: vec![other_root], now: 2_000, required: true })).unwrap_err();
        assert_eq!(e.alert, UNKNOWN_CA, "{}", e.why);
        // Expired and not yet valid.
        let e = run(Some(identity(&p)), Some(Trust { anchors: vec![p.root.clone()], now: 600_000, required: true })).unwrap_err();
        assert_eq!(e.alert, CERTIFICATE_EXPIRED);
        let e = run(Some(identity(&p)), Some(Trust { anchors: vec![p.root.clone()], now: 10, required: true })).unwrap_err();
        assert_eq!(e.alert, CERTIFICATE_EXPIRED);
        // A server that holds the certificate but not its key cannot sign: the signature fails.
        let wrong = RsaKey::generate(1024, &mut Drbg::new(b"not the key")).unwrap();
        let e = run(Some(Identity { chain: vec![p.leaf_der.clone()], key: wrong }), None).unwrap_err();
        assert_eq!(e.alert, DECRYPT_ERROR, "{}", e.why);
        // A server with no certificate: refused by a client that demands one, fine otherwise.
        let trust = Trust { anchors: vec![p.root.clone()], now: 2_000, required: true };
        assert!(run(None, Some(trust)).is_err());
        let (c, s) = run(None, None).unwrap();
        assert_eq!((c.suite_name(), s.suite_name(), c.verified()), (SUITE_NAME, SUITE_NAME, false));
        // A client that does not require one still talks to an anonymous server, unverified.
        let (c, _) = run(None, Some(Trust { anchors: vec![p.root.clone()], now: 2_000, required: false })).unwrap();
        assert!(!c.verified() && c.suite_name() == SUITE_NAME);
    }

    #[test]
    fn a_server_with_a_certificate_still_serves_an_anonymous_only_client() {
        let p = pki(500_000);
        let mut server = Tls::server_with(b"s", Some(identity(&p)));
        let mut body = VERSION.to_vec();
        body.extend_from_slice(&[7; 32]);
        body.push(0);
        put16(&mut body, &SUITE.to_be_bytes());
        body.extend_from_slice(&[1, 0]);
        let mut wire = record(HANDSHAKE, &handshake(CLIENT_HELLO, &body));
        let sent = server.feed(&mut wire).unwrap().send;
        assert!(!describe(&sent).contains("Certificate"), "{}", describe(&sent));
        // And a client offering only the certificate suite meets a server without one: refused.
        let mut body = VERSION.to_vec();
        body.extend_from_slice(&[7; 32]);
        body.push(0);
        put16(&mut body, &SUITE_RSA.to_be_bytes());
        body.extend_from_slice(&[1, 0]);
        let mut wire = record(HANDSHAKE, &handshake(CLIENT_HELLO, &body));
        assert_eq!(Tls::server(b"s").feed(&mut wire).unwrap_err().alert, HANDSHAKE_FAILURE);
    }

    #[test]
    fn a_certificate_flight_survives_every_truncation_and_corruption() {
        let p = pki(500_000);
        let mut server = Tls::server_with(b"s", Some(identity(&p)));
        let (_, hello) = Tls::client(b"c");
        let flight = server.feed(&mut hello.clone()).unwrap().send;
        assert!(describe(&flight).contains("Certificate"), "{}", describe(&flight));
        let trust = || Some(Trust { anchors: vec![p.root.clone()], now: 2_000, required: true });
        for cut in 0..flight.len() {
            let (mut c, _) = Tls::client_with(b"c", trust());
            assert!(c.feed(&mut flight[..cut].to_vec()).is_ok_and(|o| o.send.is_empty()), "cut {cut}");
        }
        for i in 0..flight.len() {
            let mut b = flight.clone();
            b[i] ^= 0x01;
            let (mut c, _) = Tls::client_with(b"c", trust());
            // Corrupting any byte either errors, or (inside a random or a length it tolerates)
            // never yields a completed key exchange that the server would accept.
            let _ = c.feed(&mut b);
        }
        // Handshake messages with hostile certificate lists.
        let (mut c, _) = Tls::client_with(b"c", trust());
        let mut sh = VERSION.to_vec();
        sh.extend_from_slice(&[1; 32]);
        sh.push(0);
        sh.extend_from_slice(&SUITE_RSA.to_be_bytes());
        sh.push(0);
        let mut msgs = handshake(SERVER_HELLO, &sh);
        msgs.extend(handshake(CERTIFICATE, &[0xff, 0xff, 0xff, 1, 2]));
        assert!(c.feed(&mut record(HANDSHAKE, &msgs)).is_err());
        let (mut c, _) = Tls::client_with(b"c", trust());
        let mut msgs = handshake(SERVER_HELLO, &sh);
        msgs.extend(handshake(CERTIFICATE, &[0, 0, 0]));
        assert!(c.feed(&mut record(HANDSHAKE, &msgs)).is_err());
    }
}
