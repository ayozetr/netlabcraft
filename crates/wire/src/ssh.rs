//! SSH 2.0 as NETCONF runs over it (RFC 6242 §3): the transport (RFC 4253) with the binary
//! packet protocol, `diffie-hellman-group14-sha256` key exchange (RFC 8268) over the RFC 3526
//! 2048-bit group, `aes128-cbc` encryption and `hmac-sha1` integrity, key derivation (§7.2);
//! password user authentication (RFC 4252 §8); and the connection protocol (RFC 4254) as far as
//! one `session` channel with the `netconf` subsystem.
//!
//! What is real: the identification strings, every packet's format, the key exchange and its
//! exchange hash, the derived keys and the encryption, so a capture shows SSH and nothing of
//! NETCONF can be read in it. What is a stand-in: the server's host key. A real server signs
//! the exchange hash with RSA, ECDSA or Ed25519; this one sends a key blob of the private
//! algorithm `netlabcraft-sim-hostkey@netlabcraft` and an HMAC of the exchange hash as its
//! "signature", which proves nothing about who answers (the client only checks it is
//! consistent). Random values come from the caller's seed.

use crate::aes::{Aes128, cbc_decrypt, cbc_encrypt};
use crate::modp;
use crate::sha1::hmac_sha1;
use crate::sha256::{hmac_sha256, sha256};
use crate::tls::Drbg;

pub const IDENT: &str = "SSH-2.0-NetLabCraft_1.0";
pub const KEX: &str = "diffie-hellman-group14-sha256";
pub const HOST_KEY: &str = "netlabcraft-sim-hostkey@netlabcraft";
pub const CIPHER: &str = "aes128-cbc";
pub const MAC: &str = "hmac-sha1";

pub const DISCONNECT: u8 = 1;
pub const IGNORE: u8 = 2;
pub const UNIMPLEMENTED: u8 = 3;
pub const DEBUG: u8 = 4;
pub const SERVICE_REQUEST: u8 = 5;
pub const SERVICE_ACCEPT: u8 = 6;
pub const KEXINIT: u8 = 20;
pub const NEWKEYS: u8 = 21;
pub const KEXDH_INIT: u8 = 30;
pub const KEXDH_REPLY: u8 = 31;
pub const USERAUTH_REQUEST: u8 = 50;
pub const USERAUTH_FAILURE: u8 = 51;
pub const USERAUTH_SUCCESS: u8 = 52;
pub const USERAUTH_BANNER: u8 = 53;
pub const CHANNEL_OPEN: u8 = 90;
pub const CHANNEL_OPEN_CONFIRMATION: u8 = 91;
pub const CHANNEL_OPEN_FAILURE: u8 = 92;
pub const CHANNEL_WINDOW_ADJUST: u8 = 93;
pub const CHANNEL_DATA: u8 = 94;
pub const CHANNEL_EOF: u8 = 96;
pub const CHANNEL_CLOSE: u8 = 97;
pub const CHANNEL_REQUEST: u8 = 98;
pub const CHANNEL_SUCCESS: u8 = 99;
pub const CHANNEL_FAILURE: u8 = 100;

/// Disconnect reasons (RFC 4250 §4.2.2).
pub const PROTOCOL_ERROR: u32 = 2;
pub const KEY_EXCHANGE_FAILED: u32 = 3;
pub const MAC_ERROR: u32 = 5;
pub const SERVICE_NOT_AVAILABLE: u32 = 7;
pub const BY_APPLICATION: u32 = 11;
pub const NO_MORE_AUTH_METHODS: u32 = 14;
/// Not a disconnect reason: the peer didn't start with an SSH identification at all.
pub const MISMATCH: u32 = 0;

/// The largest packet accepted (RFC 4253 §6.1).
pub const MAX_PACKET: usize = 35_000;
/// The most channel data one packet carries.
pub const MAX_DATA: usize = 32_768;
const WINDOW: u32 = 2 * 1024 * 1024;
const MAX_IDENT: usize = 255;
const MAX_AUTH_TRIES: u8 = 3;

/// An SSH failure: the disconnect reason to send and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshError {
    pub reason: u32,
    pub why: String,
}

fn fail(reason: u32, why: impl Into<String>) -> SshError {
    SshError { reason, why: why.into() }
}

type R<T> = std::result::Result<T, SshError>;

/// A cursor over a packet payload (RFC 4251 §5 data types).
struct Rd<'a>(&'a [u8]);

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        match (self.0.get(..n), self.0.get(n..)) {
            (Some(a), Some(b)) => {
                self.0 = b;
                Ok(a)
            }
            _ => Err(fail(PROTOCOL_ERROR, "a packet is cut short")),
        }
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> R<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn bool(&mut self) -> R<bool> {
        Ok(self.u8()? != 0)
    }
    fn string(&mut self) -> R<&'a [u8]> {
        let n = self.u32()? as usize;
        if n > MAX_PACKET {
            return Err(fail(PROTOCOL_ERROR, "a string longer than a packet"));
        }
        self.take(n)
    }
    fn text(&mut self) -> R<String> {
        String::from_utf8(self.string()?.to_vec()).map_err(|_| fail(PROTOCOL_ERROR, "a string that is not UTF-8"))
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &[u8]) {
    put_u32(out, u32::try_from(s.len()).unwrap_or(u32::MAX));
    out.extend_from_slice(s);
}

/// A positive integer as an mpint (RFC 4251 §5): minimal, with a leading zero if the top bit
/// is set.
pub fn mpint(be: &[u8]) -> Vec<u8> {
    let start = be.iter().position(|b| *b != 0).unwrap_or(be.len());
    let mut v = Vec::with_capacity(be.len() + 1);
    if be.get(start).is_some_and(|b| b & 0x80 != 0) {
        v.push(0);
    }
    v.extend_from_slice(be.get(start..).unwrap_or_default());
    let mut out = Vec::new();
    put_str(&mut out, &v);
    out
}

/// One direction's packet protection.
#[derive(Debug, Clone, Default)]
struct Dir {
    aes: Option<Aes128>,
    iv: [u8; 16],
    mac: Option<[u8; 20]>,
    seq: u32,
}

impl Dir {
    fn block(&self) -> usize {
        if self.aes.is_some() { 16 } else { 8 }
    }

    /// One packet (RFC 4253 §6): length, padding length, payload, padding, MAC.
    fn seal(&mut self, payload: &[u8], rng: &mut Drbg) -> Vec<u8> {
        let block = self.block();
        let mut pad = block - (5 + payload.len()) % block;
        if pad < 4 {
            pad += block;
        }
        let len = 1 + payload.len() + pad;
        let mut p = Vec::with_capacity(len + 4 + 20);
        put_u32(&mut p, u32::try_from(len).unwrap_or(u32::MAX));
        p.push(pad as u8);
        p.extend_from_slice(payload);
        p.extend(rng.bytes(pad));
        let mac = self.mac.map(|k| hmac_sha1(&k, &[&self.seq.to_be_bytes()[..], &p[..]].concat()));
        self.seq = self.seq.wrapping_add(1);
        let mut out = match &self.aes {
            Some(aes) => {
                let c = cbc_encrypt(aes, &self.iv, &p);
                if let Some(last) = c.get(c.len().saturating_sub(16)..) {
                    self.iv.copy_from_slice(last);
                }
                c
            }
            None => p,
        };
        if let Some(m) = mac {
            out.extend_from_slice(&m);
        }
        out
    }

    /// The first packet of `buf`: `Ok(None)` until it all arrived; its payload and length.
    fn open(&mut self, buf: &[u8]) -> R<Option<(Vec<u8>, usize)>> {
        let block = self.block();
        let Some(first) = buf.get(..block) else { return Ok(None) };
        let head = match &self.aes {
            Some(aes) => cbc_decrypt(aes, &self.iv, first),
            None => first.to_vec(),
        };
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        if !(12..=MAX_PACKET).contains(&len) || !(len + 4).is_multiple_of(block) {
            return Err(fail(PROTOCOL_ERROR, "a packet with a bad length (is this SSH, and are the keys right?)"));
        }
        let mac_len = if self.mac.is_some() { 20 } else { 0 };
        let Some(raw) = buf.get(..4 + len) else { return Ok(None) };
        let Some(mac) = buf.get(4 + len..4 + len + mac_len) else { return Ok(None) };
        let p = match &self.aes {
            Some(aes) => {
                let p = cbc_decrypt(aes, &self.iv, raw);
                if let Some(last) = raw.get(raw.len().saturating_sub(16)..) {
                    self.iv.copy_from_slice(last);
                }
                p
            }
            None => raw.to_vec(),
        };
        if let Some(k) = self.mac
            && hmac_sha1(&k, &[&self.seq.to_be_bytes()[..], &p[..]].concat()) != mac
        {
            return Err(fail(MAC_ERROR, "a packet's MAC does not match"));
        }
        self.seq = self.seq.wrapping_add(1);
        let pad = usize::from(p.get(4).copied().unwrap_or(0));
        if pad < 4 || pad + 1 > len {
            return Err(fail(PROTOCOL_ERROR, "a packet with bad padding"));
        }
        Ok(Some((p.get(5..4 + len - pad).unwrap_or_default().to_vec(), 4 + len + mac_len)))
    }
}

/// What an endpoint made of the bytes it was given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// Bytes to send.
    pub send: Vec<u8>,
    /// Channel data that arrived.
    pub app: Vec<u8>,
    /// The `netconf` subsystem is running: channel data can flow.
    pub ready: bool,
    /// The session is over (closed channel, disconnect).
    pub closed: bool,
    /// The server refused the password (client side).
    pub auth_failed: bool,
    /// Why the peer disconnected, if it said.
    pub peer_reason: Option<String>,
    /// Server side: the user who logged in, once.
    pub logged_in: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Ident,
    KexInit,
    KexDh,
    NewKeys,
    Service,
    Auth,
    Channel,
    Subsystem,
    Open,
    Closed,
}

/// Either end of an SSH session carrying one `netconf` subsystem channel.
#[derive(Debug, Clone)]
pub struct Ssh {
    server: bool,
    stage: Stage,
    rng: Drbg,
    private: Vec<u8>,
    host_key: Vec<u8>,
    v_c: Vec<u8>,
    v_s: Vec<u8>,
    i_c: Vec<u8>,
    i_s: Vec<u8>,
    e: Vec<u8>,
    session_id: Vec<u8>,
    read: Dir,
    write: Dir,
    pending: Option<(Dir, Dir)>,
    user: String,
    password: String,
    subsystem: String,
    peer_channel: u32,
    tries: u8,
}

fn kexinit(rng: &mut Drbg) -> Vec<u8> {
    let mut p = vec![KEXINIT];
    p.extend(rng.bytes(16));
    for list in [KEX, HOST_KEY, CIPHER, CIPHER, MAC, MAC, "none", "none", "", ""] {
        put_str(&mut p, list.as_bytes());
    }
    p.push(0);
    put_u32(&mut p, 0);
    p
}

/// Checks a peer's KEXINIT offers what this side speaks (one algorithm of each kind).
fn negotiate(payload: &[u8]) -> R<()> {
    let mut rd = Rd(payload);
    rd.u8()?;
    rd.take(16)?;
    for (want, what) in [
        (KEX, "key exchange"),
        (HOST_KEY, "host key"),
        (CIPHER, "cipher"),
        (CIPHER, "cipher"),
        (MAC, "MAC"),
        (MAC, "MAC"),
        ("none", "compression"),
        ("none", "compression"),
    ] {
        let list = rd.text()?;
        if !list.split(',').any(|a| a == want) {
            return Err(fail(KEY_EXCHANGE_FAILED, format!("no common {what} algorithm (this side has {want}, the peer offers {list})")));
        }
    }
    Ok(())
}

impl Ssh {
    fn new(server: bool, seed: &[u8]) -> Ssh {
        let mut rng = Drbg::new(seed);
        let private = rng.bytes(modp::PRIVATE_LEN);
        Ssh {
            server,
            stage: Stage::Ident,
            rng,
            private,
            host_key: Vec::new(),
            v_c: Vec::new(),
            v_s: Vec::new(),
            i_c: Vec::new(),
            i_s: Vec::new(),
            e: Vec::new(),
            session_id: Vec::new(),
            read: Dir::default(),
            write: Dir::default(),
            pending: None,
            user: String::new(),
            password: String::new(),
            subsystem: String::new(),
            peer_channel: 0,
            tries: 0,
        }
    }

    /// A server; `host_id` names its stand-in host key (stable per router).
    pub fn server(seed: &[u8], host_id: &[u8]) -> Ssh {
        let mut s = Ssh::new(true, seed);
        s.v_s = IDENT.as_bytes().to_vec();
        let mut blob = Vec::new();
        put_str(&mut blob, HOST_KEY.as_bytes());
        put_str(&mut blob, &sha256(host_id));
        s.host_key = blob;
        s.subsystem = "netconf".into();
        s
    }

    /// A client logging in as `user` for `subsystem`, and what it sends first (its
    /// identification and KEXINIT).
    pub fn client(seed: &[u8], user: &str, password: &str, subsystem: &str) -> (Ssh, Vec<u8>) {
        let mut c = Ssh::new(false, seed);
        c.v_c = IDENT.as_bytes().to_vec();
        c.user = user.to_string();
        c.password = password.to_string();
        c.subsystem = subsystem.to_string();
        let mut out = format!("{IDENT}\r\n").into_bytes();
        c.i_c = kexinit(&mut c.rng);
        out.extend(c.write.seal(&c.i_c.clone(), &mut c.rng));
        (c, out)
    }

    pub fn ready(&self) -> bool {
        self.stage == Stage::Open
    }

    fn packet(&mut self, payload: &[u8]) -> Vec<u8> {
        self.write.seal(payload, &mut self.rng)
    }

    /// Channel data packets.
    pub fn send(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for c in data.chunks(MAX_DATA) {
            let mut p = vec![CHANNEL_DATA];
            put_u32(&mut p, self.peer_channel);
            put_str(&mut p, c);
            out.extend(self.packet(&p));
        }
        out
    }

    /// Ends the channel (EOF and CLOSE).
    pub fn close(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.stage == Stage::Open {
            for kind in [CHANNEL_EOF, CHANNEL_CLOSE] {
                let mut p = vec![kind];
                put_u32(&mut p, self.peer_channel);
                out.extend(self.packet(&p));
            }
        }
        self.stage = Stage::Closed;
        out
    }

    /// A DISCONNECT for an error (or the plain-text mismatch line before any packet).
    pub fn disconnect(&mut self, e: &SshError) -> Vec<u8> {
        self.stage = Stage::Closed;
        if e.reason == MISMATCH {
            return b"Protocol mismatch.\r\n".to_vec();
        }
        let mut p = vec![DISCONNECT];
        put_u32(&mut p, e.reason);
        put_str(&mut p, e.why.as_bytes());
        put_str(&mut p, b"en");
        self.packet(&p)
    }

    /// The identification line at the front of `buf`; lines before it are skipped on the
    /// client side (RFC 4253 §4.2).
    fn ident(&mut self, buf: &mut Vec<u8>) -> R<bool> {
        for _ in 0..16 {
            let Some(end) = buf.iter().position(|b| *b == b'\n') else {
                if buf.len() > MAX_IDENT {
                    return Err(fail(MISMATCH, "no identification line"));
                }
                if !buf.is_empty() && !b"SSH-".starts_with(buf.get(..buf.len().min(4)).unwrap_or_default()) && self.server {
                    return Err(fail(MISMATCH, "the peer does not speak SSH"));
                }
                return Ok(false);
            };
            if end > MAX_IDENT {
                return Err(fail(MISMATCH, "an identification line longer than 255 bytes"));
            }
            let line: Vec<u8> = buf.drain(..=end).collect();
            let line = line.strip_suffix(b"\n").unwrap_or(&line);
            let line = line.strip_suffix(b"\r").unwrap_or(line).to_vec();
            if line.starts_with(b"SSH-2.0-") || line.starts_with(b"SSH-1.99-") {
                if self.server {
                    self.v_c = line;
                } else {
                    self.v_s = line;
                }
                return Ok(true);
            }
            if self.server || line.starts_with(b"SSH-") {
                return Err(fail(MISMATCH, "the peer does not speak SSH 2.0"));
            }
        }
        Err(fail(MISMATCH, "no identification line"))
    }

    fn exchange_hash(&self, k_s: &[u8], e: &[u8], f: &[u8], k: &[u8]) -> Vec<u8> {
        let mut h = Vec::new();
        for s in [&self.v_c, &self.v_s, &self.i_c, &self.i_s] {
            put_str(&mut h, s);
        }
        put_str(&mut h, k_s);
        h.extend(mpint(e));
        h.extend(mpint(f));
        h.extend(mpint(k));
        sha256(&h).to_vec()
    }

    /// The keys of both directions (RFC 4253 §7.2): (client to server, server to client).
    fn keys(&self, k: &[u8], h: &[u8]) -> (Dir, Dir) {
        let derive = |x: u8| sha256(&[&mpint(k)[..], h, &[x], &self.session_id[..]].concat());
        let arr16 = |b: [u8; 32]| {
            let mut a = [0u8; 16];
            a.copy_from_slice(&b[..16]);
            a
        };
        let arr20 = |b: [u8; 32]| {
            let mut a = [0u8; 20];
            a.copy_from_slice(&b[..20]);
            a
        };
        let c2s = Dir { aes: Some(Aes128::new(&arr16(derive(b'C')))), iv: arr16(derive(b'A')), mac: Some(arr20(derive(b'E'))), seq: 0 };
        let s2c = Dir { aes: Some(Aes128::new(&arr16(derive(b'D')))), iv: arr16(derive(b'B')), mac: Some(arr20(derive(b'F'))), seq: 0 };
        (c2s, s2c)
    }

    /// Takes the complete packets off the front of `buf` and acts on them. `auth` checks a
    /// user and password (server side).
    pub fn feed(&mut self, buf: &mut Vec<u8>, auth: &dyn Fn(&str, &str) -> bool) -> R<Output> {
        let mut out = Output::default();
        if self.stage == Stage::Ident {
            if !self.ident(buf)? {
                return Ok(out);
            }
            if self.server {
                out.send.extend(format!("{IDENT}\r\n").into_bytes());
                self.i_s = kexinit(&mut self.rng);
                let k = self.i_s.clone();
                out.send.extend(self.packet(&k));
            }
            self.stage = Stage::KexInit;
        }
        for _ in 0..256 {
            if self.stage == Stage::Closed {
                break;
            }
            let Some((payload, used)) = self.read.open(buf)? else { break };
            buf.drain(..used.min(buf.len()));
            self.packet_in(&payload, auth, &mut out)?;
        }
        out.ready = self.stage == Stage::Open;
        Ok(out)
    }

    fn packet_in(&mut self, payload: &[u8], auth: &dyn Fn(&str, &str) -> bool, out: &mut Output) -> R<()> {
        let mut rd = Rd(payload);
        let kind = rd.u8()?;
        match kind {
            IGNORE | DEBUG | UNIMPLEMENTED | USERAUTH_BANNER | CHANNEL_WINDOW_ADJUST => return Ok(()),
            DISCONNECT => {
                let _ = rd.u32();
                out.peer_reason = rd.text().ok();
                out.closed = true;
                self.stage = Stage::Closed;
                return Ok(());
            }
            _ => {}
        }
        match (self.server, self.stage, kind) {
            (_, Stage::KexInit, KEXINIT) => {
                negotiate(payload)?;
                if self.server {
                    self.i_c = payload.to_vec();
                } else {
                    self.i_s = payload.to_vec();
                    self.e = modp::public(&self.private);
                    let mut p = vec![KEXDH_INIT];
                    p.extend(mpint(&self.e));
                    out.send.extend(self.packet(&p));
                }
                self.stage = Stage::KexDh;
            }
            (true, Stage::KexDh, KEXDH_INIT) => {
                let e = rd.string()?.to_vec();
                let k = modp::shared(&e, &self.private).ok_or_else(|| fail(KEY_EXCHANGE_FAILED, "the client's public value is unusable"))?;
                let f = modp::public(&self.private);
                let h = self.exchange_hash(&self.host_key.clone(), &e, &f, &k);
                self.session_id = h.clone();
                let mut sig = Vec::new();
                put_str(&mut sig, HOST_KEY.as_bytes());
                put_str(&mut sig, &hmac_sha256(&self.host_key, &h));
                let mut p = vec![KEXDH_REPLY];
                put_str(&mut p, &self.host_key.clone());
                p.extend(mpint(&f));
                put_str(&mut p, &sig);
                out.send.extend(self.packet(&p));
                out.send.extend(self.packet(&[NEWKEYS]));
                let (c2s, s2c) = self.keys(&k, &h);
                // Sequence numbers run on across the key change (RFC 4253 §6.4).
                let wseq = self.write.seq;
                self.write = Dir { seq: wseq, ..s2c };
                self.pending = Some((c2s, Dir::default()));
                self.stage = Stage::NewKeys;
            }
            (false, Stage::KexDh, KEXDH_REPLY) => {
                let k_s = rd.string()?.to_vec();
                let f = rd.string()?.to_vec();
                let sig = rd.string()?.to_vec();
                let mut ks = Rd(&k_s);
                if ks.string()? != HOST_KEY.as_bytes() {
                    return Err(fail(KEY_EXCHANGE_FAILED, "the server's host key is of an unknown kind"));
                }
                let k = modp::shared(&f, &self.private).ok_or_else(|| fail(KEY_EXCHANGE_FAILED, "the server's public value is unusable"))?;
                let h = self.exchange_hash(&k_s, &self.e.clone(), &f, &k);
                let mut sr = Rd(&sig);
                if sr.string()? != HOST_KEY.as_bytes() || sr.string()? != hmac_sha256(&k_s, &h) {
                    return Err(fail(KEY_EXCHANGE_FAILED, "the server's exchange-hash check does not match"));
                }
                self.session_id = h.clone();
                self.pending = Some(self.keys(&k, &h));
                self.stage = Stage::NewKeys;
            }
            (_, Stage::NewKeys, NEWKEYS) => {
                let (c2s, s2c) = self.pending.take().ok_or_else(|| fail(PROTOCOL_ERROR, "NEWKEYS before the key exchange"))?;
                let seq = self.read.seq;
                if self.server {
                    self.read = Dir { seq, ..c2s };
                    self.stage = Stage::Service;
                } else {
                    self.read = Dir { seq, ..s2c };
                    out.send.extend(self.packet(&[NEWKEYS]));
                    let wseq = self.write.seq;
                    self.write = Dir { seq: wseq, ..c2s };
                    let mut p = vec![SERVICE_REQUEST];
                    put_str(&mut p, b"ssh-userauth");
                    out.send.extend(self.packet(&p));
                    self.stage = Stage::Service;
                }
            }
            (true, Stage::Service, SERVICE_REQUEST) => {
                let name = rd.string()?;
                if name != b"ssh-userauth" {
                    return Err(fail(SERVICE_NOT_AVAILABLE, "only ssh-userauth is offered"));
                }
                let mut p = vec![SERVICE_ACCEPT];
                put_str(&mut p, name);
                out.send.extend(self.packet(&p));
                self.stage = Stage::Auth;
            }
            (false, Stage::Service, SERVICE_ACCEPT) => {
                let mut p = vec![USERAUTH_REQUEST];
                put_str(&mut p, self.user.as_bytes());
                put_str(&mut p, b"ssh-connection");
                put_str(&mut p, b"password");
                p.push(0);
                put_str(&mut p, self.password.as_bytes());
                out.send.extend(self.packet(&p));
                self.stage = Stage::Auth;
            }
            (true, Stage::Auth, USERAUTH_REQUEST) => {
                let user = rd.text()?;
                let service = rd.string()?.to_vec();
                let method = rd.string()?.to_vec();
                let ok = method == b"password" && service == b"ssh-connection" && {
                    let _change = rd.bool()?;
                    let pw = rd.text()?;
                    auth(&user, &pw)
                };
                if ok {
                    out.send.extend(self.packet(&[USERAUTH_SUCCESS]));
                    out.logged_in = Some(user.clone());
                    self.user = user;
                    self.stage = Stage::Channel;
                } else {
                    if method == b"password" {
                        self.tries = self.tries.saturating_add(1);
                    }
                    if self.tries >= MAX_AUTH_TRIES {
                        return Err(fail(NO_MORE_AUTH_METHODS, "too many wrong passwords"));
                    }
                    let mut p = vec![USERAUTH_FAILURE];
                    put_str(&mut p, b"password");
                    p.push(0);
                    out.send.extend(self.packet(&p));
                }
            }
            (false, Stage::Auth, USERAUTH_SUCCESS) => {
                let mut p = vec![CHANNEL_OPEN];
                put_str(&mut p, b"session");
                put_u32(&mut p, 0);
                put_u32(&mut p, WINDOW);
                put_u32(&mut p, MAX_DATA as u32);
                out.send.extend(self.packet(&p));
                self.stage = Stage::Channel;
            }
            (false, Stage::Auth, USERAUTH_FAILURE) => {
                out.auth_failed = true;
                out.closed = true;
                let e = fail(BY_APPLICATION, "authentication failed");
                out.send.extend(self.disconnect(&e));
            }
            (true, Stage::Channel, CHANNEL_OPEN) => {
                let kind = rd.string()?.to_vec();
                let sender = rd.u32()?;
                self.peer_channel = sender;
                if kind != b"session" {
                    let mut p = vec![CHANNEL_OPEN_FAILURE];
                    put_u32(&mut p, sender);
                    put_u32(&mut p, 3);
                    put_str(&mut p, b"only session channels");
                    put_str(&mut p, b"en");
                    out.send.extend(self.packet(&p));
                    return Ok(());
                }
                let mut p = vec![CHANNEL_OPEN_CONFIRMATION];
                put_u32(&mut p, sender);
                put_u32(&mut p, 0);
                put_u32(&mut p, WINDOW);
                put_u32(&mut p, MAX_DATA as u32);
                out.send.extend(self.packet(&p));
                self.stage = Stage::Subsystem;
            }
            (false, Stage::Channel, CHANNEL_OPEN_CONFIRMATION) => {
                rd.u32()?;
                self.peer_channel = rd.u32()?;
                let mut p = vec![CHANNEL_REQUEST];
                put_u32(&mut p, self.peer_channel);
                put_str(&mut p, b"subsystem");
                p.push(1);
                put_str(&mut p, self.subsystem.clone().as_bytes());
                out.send.extend(self.packet(&p));
                self.stage = Stage::Subsystem;
            }
            (false, Stage::Channel, CHANNEL_OPEN_FAILURE) => return Err(fail(BY_APPLICATION, "the server refused the session channel")),
            (true, Stage::Subsystem | Stage::Open, CHANNEL_REQUEST) => {
                rd.u32()?;
                let kind = rd.string()?.to_vec();
                let want_reply = rd.bool()?;
                let name = if kind == b"subsystem" { rd.string()?.to_vec() } else { Vec::new() };
                let ok = self.stage == Stage::Subsystem && kind == b"subsystem" && name == self.subsystem.as_bytes();
                if want_reply || ok {
                    let mut p = vec![if ok { CHANNEL_SUCCESS } else { CHANNEL_FAILURE }];
                    put_u32(&mut p, self.peer_channel);
                    out.send.extend(self.packet(&p));
                }
                if ok {
                    self.stage = Stage::Open;
                }
            }
            (false, Stage::Subsystem, CHANNEL_SUCCESS) => self.stage = Stage::Open,
            (false, Stage::Subsystem, CHANNEL_FAILURE) => {
                return Err(fail(BY_APPLICATION, format!("the server has no {} subsystem", self.subsystem)));
            }
            (_, Stage::Open, CHANNEL_DATA) => {
                rd.u32()?;
                let data = rd.string()?;
                if out.app.len().saturating_add(data.len()) > 8 * MAX_PACKET * 32 {
                    return Err(fail(PROTOCOL_ERROR, "too much channel data at once"));
                }
                out.app.extend_from_slice(data);
            }
            (_, Stage::Open, CHANNEL_EOF) => {}
            (_, _, CHANNEL_CLOSE) => {
                if self.stage == Stage::Open {
                    let mut p = vec![CHANNEL_CLOSE];
                    put_u32(&mut p, self.peer_channel);
                    out.send.extend(self.packet(&p));
                }
                out.closed = true;
                self.stage = Stage::Closed;
            }
            (_, _, k) => return Err(fail(PROTOCOL_ERROR, format!("message {k} out of place"))),
        }
        Ok(())
    }
}

/// A short description of a segment's SSH content, for the event list: the identification
/// and the clear-text key exchange packets by name, the rest as encrypted packets.
pub fn describe(payload: &[u8]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut rest = payload;
    if rest.starts_with(b"SSH-") {
        let end = rest.iter().position(|b| *b == b'\n').map_or(rest.len(), |i| i + 1);
        let line = String::from_utf8_lossy(rest.get(..end).unwrap_or_default()).trim().chars().take(40).collect::<String>();
        parts.push(format!("identification {line}"));
        rest = rest.get(end..).unwrap_or_default();
    } else if rest.starts_with(b"Protocol mismatch") {
        return "protocol mismatch".into();
    }
    let mut clear = Dir::default();
    while !rest.is_empty() && parts.len() < 6 {
        match clear.open(rest) {
            Ok(Some((p, used))) if p.first().is_some_and(|k| [KEXINIT, NEWKEYS, KEXDH_INIT, KEXDH_REPLY, DISCONNECT].contains(k)) => {
                parts.push(
                    match p.first().copied().unwrap_or(0) {
                        KEXINIT => "KEXINIT",
                        NEWKEYS => "NEWKEYS",
                        KEXDH_INIT => "KEXDH_INIT (Diffie-Hellman)",
                        KEXDH_REPLY => "KEXDH_REPLY (Diffie-Hellman)",
                        _ => "DISCONNECT",
                    }
                    .to_string(),
                );
                rest = rest.get(used..).unwrap_or_default();
            }
            _ => {
                parts.push(format!("encrypted packets, {} bytes", rest.len()));
                break;
            }
        }
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(server: &mut Ssh, client: &mut Ssh, mut to_server: Vec<u8>, check: &dyn Fn(&str, &str) -> bool) -> (Output, Output) {
        let (mut last_s, mut last_c) = (Output::default(), Output::default());
        for _ in 0..12 {
            if to_server.is_empty() {
                break;
            }
            let s = server.feed(&mut to_server, check).unwrap();
            let mut to_client = s.send.clone();
            last_s = s;
            let c = client.feed(&mut to_client, &|_, _| false).unwrap();
            to_server = c.send.clone();
            let (failed, closed) = (last_c.auth_failed || c.auth_failed, last_c.closed || c.closed);
            last_c = Output { auth_failed: failed, closed, ..c };
        }
        (last_s, last_c)
    }

    #[test]
    fn a_session_opens_the_netconf_subsystem_and_carries_data() {
        let mut server = Ssh::server(b"srv", b"R1");
        let (mut client, first) = Ssh::client(b"cli", "admin", "pw", "netconf");
        assert!(first.starts_with(b"SSH-2.0-NetLabCraft_1.0\r\n"));
        assert!(describe(&first).contains("KEXINIT"));
        let (_, c) = run(&mut server, &mut client, first, &|u, p| u == "admin" && p == "pw");
        assert!(client.ready() && server.ready() && c.ready);
        let mut wire = client.send(b"<hello/>]]>]]>");
        assert!(!wire.windows(5).any(|w| w == b"hello"), "channel data is encrypted");
        assert!(describe(&wire).contains("encrypted"));
        let o = server.feed(&mut wire, &|_, _| false).unwrap();
        assert_eq!(o.app, b"<hello/>]]>]]>");
        let big = vec![b'r'; MAX_DATA * 2 + 9];
        let mut wire = server.send(&big);
        assert_eq!(client.feed(&mut wire, &|_, _| false).unwrap().app, big);
        let mut wire = client.close();
        let o = server.feed(&mut wire, &|_, _| false).unwrap();
        assert!(o.closed);
    }

    #[test]
    fn wrong_passwords_and_plain_text_are_refused() {
        let mut server = Ssh::server(b"srv", b"R1");
        let (mut client, first) = Ssh::client(b"cli", "admin", "nope", "netconf");
        let (_, c) = run(&mut server, &mut client, first, &|_, _| false);
        assert!(c.auth_failed && c.closed && !client.ready());
        // The old clear-text preamble, or HTTP, on the SSH port.
        for junk in [&b"user admin pw\r\n"[..], b"GET / HTTP/1.1\r\n"] {
            let mut s = Ssh::server(b"srv", b"R1");
            let e = s.feed(&mut junk.to_vec(), &|_, _| true).unwrap_err();
            assert_eq!(e.reason, MISMATCH);
            assert_eq!(s.disconnect(&e), b"Protocol mismatch.\r\n");
        }
        // A tampered encrypted packet is a MAC error.
        let mut server = Ssh::server(b"srv", b"R1");
        let (mut client, first) = Ssh::client(b"cli", "admin", "pw", "netconf");
        run(&mut server, &mut client, first, &|_, _| true);
        let mut wire = client.send(b"<rpc/>");
        let n = wire.len();
        wire[n - 25] ^= 4;
        assert_eq!(server.feed(&mut wire, &|_, _| true).unwrap_err().reason, MAC_ERROR);
    }

    #[test]
    fn every_truncation_and_corruption_is_survived() {
        let (_, first) = Ssh::client(b"cli", "admin", "pw", "netconf");
        for cut in 0..first.len() {
            let mut s = Ssh::server(b"srv", b"R1");
            let o = s.feed(&mut first[..cut].to_vec(), &|_, _| true);
            assert!(o.is_ok_and(|o| o.send.is_empty() || cut >= first.iter().position(|b| *b == b'\n').unwrap_or(0)), "cut {cut}");
        }
        let mut s = Ssh::server(b"srv", b"R1");
        let reply = s.feed(&mut first.clone(), &|_, _| true).unwrap().send;
        for cut in 0..reply.len() {
            let (mut c, _) = Ssh::client(b"cli", "admin", "pw", "netconf");
            assert!(c.feed(&mut reply[..cut].to_vec(), &|_, _| false).is_ok(), "cut {cut}");
        }
        for i in 0..reply.len().min(500) {
            let mut b = reply.clone();
            b[i] ^= 0x41;
            let (mut c, _) = Ssh::client(b"cli", "admin", "pw", "netconf");
            let _ = c.feed(&mut b, &|_, _| false);
        }
        assert_eq!(mpint(&[0x00, 0x80]), vec![0, 0, 0, 2, 0, 0x80]);
        assert_eq!(mpint(&[0, 0, 0x7f]), vec![0, 0, 0, 1, 0x7f]);
        assert_eq!(mpint(&[0]), vec![0, 0, 0, 0]);
    }
}
