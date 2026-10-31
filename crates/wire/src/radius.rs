//! RADIUS (RFC 2865): a router asks an AAA server whether a login is good. Access-Request
//! carries the user name and the password hidden with the shared secret; the server answers
//! Access-Accept or Access-Reject, signed with the same secret.
//!
//! For 802.1X logins RADIUS carries EAP instead (RFC 3579): each EAP packet travels in
//! EAP-Message attributes, every packet is signed with a Message-Authenticator (HMAC-MD5 over
//! the whole packet), the server keeps its place with Access-Challenge and the State attribute,
//! and its Access-Accept hands the authenticator the session key in an MPPE key attribute
//! (RFC 2548), salted and encrypted with the shared secret.

use std::net::Ipv4Addr;

use crate::md5::md5;
use crate::{Result, WireError, be16};

pub const RADIUS_PORT: u16 = 1812;
pub const ACCESS_REQUEST: u8 = 1;
pub const ACCESS_ACCEPT: u8 = 2;
pub const ACCESS_REJECT: u8 = 3;
const USER_NAME: u8 = 1;
const USER_PASSWORD: u8 = 2;
const NAS_IP: u8 = 4;
const REPLY_MESSAGE: u8 = 18;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Radius {
    pub code: u8,
    pub id: u8,
    pub authenticator: [u8; 16],
    pub attributes: Vec<(u8, Vec<u8>)>,
}

/// The password hidden for the wire (RFC 2865 §5.2).
pub fn hide_password(password: &str, secret: &str, request_auth: &[u8; 16]) -> Vec<u8> {
    let mut p = password.as_bytes().to_vec();
    p.truncate(128);
    let len = p.len().div_ceil(16).max(1) * 16;
    p.resize(len, 0);
    let mut out = Vec::with_capacity(len);
    let mut prev = request_auth.to_vec();
    for block in p.chunks(16) {
        let b = md5(&[secret.as_bytes(), &prev].concat());
        let c: Vec<u8> = block.iter().zip(b).map(|(x, y)| x ^ y).collect();
        out.extend_from_slice(&c);
        prev = c;
    }
    out
}

/// The password a request carries, as the server reads it with the secret.
pub fn reveal_password(hidden: &[u8], secret: &str, request_auth: &[u8; 16]) -> String {
    let mut out = Vec::new();
    let mut prev = request_auth.to_vec();
    for block in hidden.chunks(16) {
        let b = md5(&[secret.as_bytes(), &prev].concat());
        out.extend(block.iter().zip(b).map(|(x, y)| x ^ y));
        prev = block.to_vec();
    }
    while out.last() == Some(&0) {
        out.pop();
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Radius {
    /// An Access-Request for a login.
    pub fn request(id: u8, authenticator: [u8; 16], user: &str, password: &str, secret: &str, nas: Ipv4Addr) -> Radius {
        Radius {
            code: ACCESS_REQUEST,
            id,
            authenticator,
            attributes: vec![
                (USER_NAME, user.as_bytes().iter().take(253).copied().collect()),
                (USER_PASSWORD, hide_password(password, secret, &authenticator)),
                (NAS_IP, nas.octets().to_vec()),
            ],
        }
    }

    /// The server's answer, signed: MD5 over the packet with the request's authenticator, then
    /// the secret.
    pub fn answer(request: &Radius, accept: bool, message: &str, secret: &str) -> Radius {
        let mut r = Radius {
            code: if accept { ACCESS_ACCEPT } else { ACCESS_REJECT },
            id: request.id,
            authenticator: request.authenticator,
            attributes: vec![(REPLY_MESSAGE, message.as_bytes().iter().take(253).copied().collect())],
        };
        r.authenticator = md5(&[r.encode(), secret.as_bytes().to_vec()].concat());
        r
    }

    /// Whether an answer is signed with `secret` for the request that had `request_auth`.
    pub fn verifies(&self, request_auth: &[u8; 16], secret: &str) -> bool {
        let unsigned = Radius { authenticator: *request_auth, ..self.clone() };
        md5(&[unsigned.encode(), secret.as_bytes().to_vec()].concat()) == self.authenticator
    }

    pub fn attribute(&self, kind: u8) -> Option<&[u8]> {
        self.attributes.iter().find(|(k, _)| *k == kind).map(|(_, v)| v.as_slice())
    }

    pub fn user(&self) -> Option<String> {
        self.attribute(USER_NAME).map(|v| String::from_utf8_lossy(v).into_owned())
    }

    pub fn hidden_password(&self) -> Option<&[u8]> {
        self.attribute(USER_PASSWORD)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = vec![self.code, self.id, 0, 0];
        v.extend_from_slice(&self.authenticator);
        for (k, val) in &self.attributes {
            let val = val.get(..val.len().min(253)).unwrap_or_default();
            v.push(*k);
            v.push(u8::try_from(val.len() + 2).unwrap_or(255));
            v.extend_from_slice(val);
        }
        let len = u16::try_from(v.len()).unwrap_or(u16::MAX).to_be_bytes();
        if let Some(s) = v.get_mut(2..4) {
            s.copy_from_slice(&len);
        }
        v
    }

    pub fn decode(b: &[u8]) -> Result<Radius> {
        let t = || WireError::Truncated("radius");
        let len = usize::from(be16(b, 2).ok_or_else(t)?);
        let p = b.get(..len).filter(|_| len >= 20).ok_or_else(t)?;
        let mut attributes = Vec::new();
        let mut at = 20;
        while at < len {
            let kind = *p.get(at).ok_or_else(t)?;
            let l = usize::from(*p.get(at + 1).ok_or_else(t)?);
            if l < 2 {
                return Err(WireError::Unsupported("radius", "attribute length".into()));
            }
            attributes.push((kind, p.get(at + 2..at + l).ok_or_else(t)?.to_vec()));
            at += l;
        }
        Ok(Radius {
            code: *p.first().ok_or_else(t)?,
            id: *p.get(1).ok_or_else(t)?,
            authenticator: p.get(4..20).and_then(|a| a.try_into().ok()).ok_or_else(t)?,
            attributes,
        })
    }

    pub fn name(&self) -> &'static str {
        match self.code {
            ACCESS_REQUEST => "Access-Request",
            ACCESS_ACCEPT => "Access-Accept",
            ACCESS_REJECT => "Access-Reject",
            ACCESS_CHALLENGE => "Access-Challenge",
            _ => "message",
        }
    }
}

// ---- EAP over RADIUS (RFC 3579) and MPPE keys (RFC 2548) ----

/// RADIUS Access-Challenge: the server wants another round (an EAP exchange under way).
pub const ACCESS_CHALLENGE: u8 = 11;
pub const STATE: u8 = 24;
pub const VENDOR_SPECIFIC: u8 = 26;
/// EAP-Message: an EAP packet, split across as many attributes as it takes (RFC 3579 §3.1).
pub const EAP_MESSAGE: u8 = 79;
/// Message-Authenticator: HMAC-MD5 over the whole packet (RFC 3579 §3.2).
pub const MESSAGE_AUTHENTICATOR: u8 = 80;
/// Microsoft's vendor id and its MPPE key attributes (RFC 2548 §2.4).
pub const VENDOR_MICROSOFT: u32 = 311;
pub const MS_MPPE_SEND_KEY: u8 = 16;
pub const MS_MPPE_RECV_KEY: u8 = 17;
/// Longest EAP packet carried (hostile input).
pub const MAX_EAP: usize = 4096;

fn hmac_md5(key: &[u8], data: &[u8]) -> [u8; 16] {
    let h = crate::sha1::hmac(crate::sha1::Hash::Md5, key, data);
    let mut out = [0u8; 16];
    for (o, b) in out.iter_mut().zip(h) {
        *o = b;
    }
    out
}

/// The salted encryption of an MPPE key (RFC 2548 §2.4.2-3): a length byte, the key, padding
/// to whole 16-byte blocks, each block XORed with MD5(secret + previous).
fn mppe_crypt(secret: &str, request_auth: &[u8; 16], salt: [u8; 2], data: &[u8], decrypt: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev: Vec<u8> = [&request_auth[..], &salt[..]].concat();
    for block in data.chunks(16) {
        let b = md5(&[secret.as_bytes(), &prev].concat());
        let c: Vec<u8> = block.iter().zip(b).map(|(x, y)| x ^ y).collect();
        prev = if decrypt { block.to_vec() } else { c.clone() };
        out.extend_from_slice(&c);
    }
    out
}

impl Radius {
    /// An Access-Request carrying an EAP packet (and the server's State when it answered
    /// before), with its Message-Authenticator.
    pub fn eap_request(id: u8, authenticator: [u8; 16], user: &str, eap: &[u8], state: Option<&[u8]>, secret: &str, nas: Ipv4Addr) -> Radius {
        let mut r = Radius {
            code: ACCESS_REQUEST,
            id,
            authenticator,
            attributes: vec![(USER_NAME, user.as_bytes().iter().take(253).copied().collect()), (NAS_IP, nas.octets().to_vec())],
        };
        r.push_eap(eap);
        if let Some(s) = state {
            r.attributes.push((STATE, s.iter().take(253).copied().collect()));
        }
        r.sign_message(&authenticator, secret);
        r
    }

    /// The server's answer to an EAP request: Access-Challenge (with State), Access-Accept
    /// (with the MPPE receive key) or Access-Reject, carrying an EAP packet, with its
    /// Message-Authenticator and then its Response Authenticator.
    pub fn eap_answer(request: &Radius, code: u8, eap: &[u8], state: Option<&[u8]>, key: Option<(&[u8], [u8; 2])>, secret: &str) -> Radius {
        let mut r = Radius { code, id: request.id, authenticator: request.authenticator, attributes: Vec::new() };
        r.push_eap(eap);
        if let Some(s) = state {
            r.attributes.push((STATE, s.iter().take(253).copied().collect()));
        }
        if let Some((k, salt)) = key {
            r.attributes.push((VENDOR_SPECIFIC, mppe_attribute(MS_MPPE_RECV_KEY, k, secret, &request.authenticator, salt)));
        }
        r.sign_message(&request.authenticator, secret);
        r.authenticator = md5(&[r.encode(), secret.as_bytes().to_vec()].concat());
        r
    }

    fn push_eap(&mut self, eap: &[u8]) {
        for chunk in eap.get(..eap.len().min(MAX_EAP)).unwrap_or_default().chunks(253) {
            self.attributes.push((EAP_MESSAGE, chunk.to_vec()));
        }
    }

    /// The Message-Authenticator this packet should carry: HMAC-MD5 with the secret over the
    /// packet, with `request_auth` in the authenticator field and the attribute zeroed.
    pub fn message_authenticator(&self, request_auth: &[u8; 16], secret: &str) -> [u8; 16] {
        let mut p = Radius { authenticator: *request_auth, ..self.clone() };
        for (k, v) in &mut p.attributes {
            if *k == MESSAGE_AUTHENTICATOR {
                *v = vec![0; 16];
            }
        }
        hmac_md5(secret.as_bytes(), &p.encode())
    }

    /// Adds (or replaces) the Message-Authenticator.
    fn sign_message(&mut self, request_auth: &[u8; 16], secret: &str) {
        self.attributes.retain(|(k, _)| *k != MESSAGE_AUTHENTICATOR);
        self.attributes.push((MESSAGE_AUTHENTICATOR, vec![0; 16]));
        let mac = self.message_authenticator(request_auth, secret);
        if let Some((_, v)) = self.attributes.iter_mut().find(|(k, _)| *k == MESSAGE_AUTHENTICATOR) {
            *v = mac.to_vec();
        }
    }

    /// Whether the packet carries one Message-Authenticator and it checks out with `secret`
    /// (`request_auth`: the request's authenticator; for a request, its own).
    pub fn checks_message(&self, request_auth: &[u8; 16], secret: &str) -> bool {
        let mut found = self.attributes.iter().filter(|(k, _)| *k == MESSAGE_AUTHENTICATOR);
        match (found.next(), found.next()) {
            (Some((_, v)), None) => v.len() == 16 && *v == self.message_authenticator(request_auth, secret),
            _ => false,
        }
    }

    /// The EAP packet the EAP-Message attributes carry, joined in order.
    pub fn eap_message(&self) -> Option<Vec<u8>> {
        let mut v = Vec::new();
        let mut any = false;
        for (_, part) in self.attributes.iter().filter(|(k, _)| *k == EAP_MESSAGE) {
            any = true;
            if v.len() + part.len() > MAX_EAP {
                break;
            }
            v.extend_from_slice(part);
        }
        any.then_some(v)
    }

    pub fn state(&self) -> Option<&[u8]> {
        self.attribute(STATE)
    }

    /// The MPPE receive key of an Access-Accept, decrypted with the secret and the request's
    /// authenticator.
    pub fn mppe_recv_key(&self, request_auth: &[u8; 16], secret: &str) -> Option<Vec<u8>> {
        self.attributes.iter().filter(|(k, _)| *k == VENDOR_SPECIFIC).find_map(|(_, v)| {
            let vendor = u32::from_be_bytes(v.get(..4)?.try_into().ok()?);
            if vendor != VENDOR_MICROSOFT || *v.get(4)? != MS_MPPE_RECV_KEY {
                return None;
            }
            let vlen = usize::from(*v.get(5)?);
            let body = v.get(6..4 + vlen)?;
            let salt: [u8; 2] = body.get(..2)?.try_into().ok()?;
            let plain = mppe_crypt(secret, request_auth, salt, body.get(2..)?, true);
            let n = usize::from(*plain.first()?);
            plain.get(1..1 + n).map(<[u8]>::to_vec)
        })
    }
}

/// A Vendor-Specific attribute's value holding one MPPE key (RFC 2548 §2.4.2-3).
fn mppe_attribute(kind: u8, key: &[u8], secret: &str, request_auth: &[u8; 16], salt: [u8; 2]) -> Vec<u8> {
    let key = key.get(..key.len().min(64)).unwrap_or_default();
    let mut plain = vec![key.len() as u8];
    plain.extend_from_slice(key);
    plain.resize(plain.len().div_ceil(16) * 16, 0);
    let salt = [salt[0] | 0x80, salt[1]];
    let mut v = VENDOR_MICROSOFT.to_be_bytes().to_vec();
    let crypt = mppe_crypt(secret, request_auth, salt, &plain, false);
    v.push(kind);
    v.push((2 + 2 + crypt.len()) as u8);
    v.extend_from_slice(&salt);
    v.extend(crypt);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_answer_round_trip_and_verify() {
        let auth = [7u8; 16];
        let req = Radius::request(9, auth, "admin", "a long password, 26 bytes", "s3cret", Ipv4Addr::new(10, 0, 0, 1));
        let back = Radius::decode(&req.encode()).unwrap();
        assert_eq!(back, req);
        assert_eq!(back.user().as_deref(), Some("admin"));
        assert_eq!(back.hidden_password().unwrap().len(), 32);
        assert_eq!(reveal_password(back.hidden_password().unwrap(), "s3cret", &auth), "a long password, 26 bytes");
        assert_ne!(reveal_password(back.hidden_password().unwrap(), "wrong", &auth), "a long password, 26 bytes");
        let ok = Radius::answer(&req, true, "Welcome", "s3cret");
        let ok = Radius::decode(&ok.encode()).unwrap();
        assert!(ok.verifies(&auth, "s3cret") && !ok.verifies(&auth, "other"));
        assert_eq!(ok.name(), "Access-Accept");
        let b = req.encode();
        for k in 0..b.len() {
            let _ = Radius::decode(&b[..k]);
        }
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// HMAC-MD5 as RFC 2202 §2 has it (test cases 1 and 2), the hash the Message-Authenticator uses.
    #[test]
    fn hmac_md5_matches_rfc_2202() {
        assert_eq!(hmac_md5(&[0x0b; 16], b"Hi There").to_vec(), hex("9294727a3638bb1c13f48ef8158bfc9d"));
        assert_eq!(hmac_md5(b"Jefe", b"what do ya want for nothing?").to_vec(), hex("750c783e6ab0b503eaa86e310a5db738"));
    }

    /// RFC 3579 §3.2: the Message-Authenticator is HMAC-MD5 with the secret over the packet as
    /// sent, its own 16 bytes zeroed; in an answer, over the packet with the request's
    /// authenticator in place of its own, and before the Response Authenticator is computed.
    #[test]
    fn eap_over_radius_follows_rfc_3579() {
        let auth = [0x5a; 16];
        let eap = [vec![2, 7, 1, 45, 25], vec![0xee; 296]].concat();
        let nas = Ipv4Addr::new(192, 168, 20, 2);
        let req = Radius::eap_request(3, auth, "ana", &eap, Some(b"cookie"), "wlcsecret", nas);
        let raw = req.encode();
        // Recomputed by hand from the bytes on the wire.
        let at = raw.windows(2).position(|w| w == [MESSAGE_AUTHENTICATOR, 18]).unwrap();
        let mut zeroed = raw.clone();
        zeroed[at + 2..at + 18].fill(0);
        assert_eq!(hmac_md5(b"wlcsecret", &zeroed).to_vec(), raw[at + 2..at + 18].to_vec());
        let back = Radius::decode(&raw).unwrap();
        assert!(back.checks_message(&auth, "wlcsecret"));
        assert!(!back.checks_message(&auth, "other"));
        // The EAP packet split over two attributes (253 + 48 bytes), joined back in order.
        assert_eq!(back.attributes.iter().filter(|(k, _)| *k == EAP_MESSAGE).map(|(_, v)| v.len()).collect::<Vec<_>>(), vec![253, 48]);
        assert_eq!(back.eap_message(), Some(eap.clone()));
        assert_eq!(back.state(), Some(&b"cookie"[..]));
        assert_eq!(back.user().as_deref(), Some("ana"));
        // A changed byte anywhere breaks it.
        let mut bad = raw.clone();
        let n = bad.len() - 30;
        bad[n] ^= 1;
        assert!(!Radius::decode(&bad).unwrap().checks_message(&auth, "wlcsecret"));
        // No Message-Authenticator, or two: refused.
        let mut none = back.clone();
        none.attributes.retain(|(k, _)| *k != MESSAGE_AUTHENTICATOR);
        assert!(!none.checks_message(&auth, "wlcsecret"));
        let mut two = back.clone();
        two.attributes.push((MESSAGE_AUTHENTICATOR, vec![0; 16]));
        assert!(!two.checks_message(&auth, "wlcsecret"));
        // The answers: challenge with State, accept with the MPPE key, reject.
        let ch =
            Radius::decode(&Radius::eap_answer(&req, ACCESS_CHALLENGE, &[1, 8, 0, 6, 25, 0x20], Some(b"s1"), None, "wlcsecret").encode()).unwrap();
        assert_eq!(ch.name(), "Access-Challenge");
        assert!(ch.verifies(&auth, "wlcsecret") && ch.checks_message(&auth, "wlcsecret"));
        // Its Message-Authenticator is over the packet with the request's authenticator.
        let mut with_req = ch.clone();
        with_req.authenticator = auth;
        let raw_ch = with_req.encode();
        let at = raw_ch.windows(2).position(|w| w == [MESSAGE_AUTHENTICATOR, 18]).unwrap();
        let mut z = raw_ch.clone();
        z[at + 2..at + 18].fill(0);
        assert_eq!(hmac_md5(b"wlcsecret", &z).to_vec(), raw_ch[at + 2..at + 18].to_vec());
        assert_eq!(ch.state(), Some(&b"s1"[..]));
        let key = [0x42u8; 32];
        let ok = Radius::decode(&Radius::eap_answer(&req, ACCESS_ACCEPT, &[3, 9, 0, 4], None, Some((&key, [1, 2])), "wlcsecret").encode()).unwrap();
        assert!(ok.verifies(&auth, "wlcsecret") && ok.checks_message(&auth, "wlcsecret"));
        assert_eq!(ok.mppe_recv_key(&auth, "wlcsecret"), Some(key.to_vec()));
        assert_ne!(ok.mppe_recv_key(&auth, "other"), Some(key.to_vec()));
        // The key is not in the clear.
        assert!(!ok.encode().windows(32).any(|w| w == key));
        let no = Radius::decode(&Radius::eap_answer(&req, ACCESS_REJECT, &[4, 9, 0, 4], None, None, "wlcsecret").encode()).unwrap();
        assert_eq!((no.name(), no.eap_message()), ("Access-Reject", Some(vec![4, 9, 0, 4])));
        for k in 0..raw.len() {
            if let Ok(r) = Radius::decode(&raw[..k]) {
                let _ = (r.eap_message(), r.checks_message(&auth, "x"), r.mppe_recv_key(&auth, "x"));
            }
        }
        let raw_ok = ok.encode();
        for k in 0..raw_ok.len() {
            if let Ok(r) = Radius::decode(&raw_ok[..k]) {
                let _ = r.mppe_recv_key(&auth, "wlcsecret");
            }
        }
    }
}
