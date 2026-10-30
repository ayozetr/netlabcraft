//! RADIUS (RFC 2865): a router asks an AAA server whether a login is good. Access-Request
//! carries the user name and the password hidden with the shared secret; the server answers
//! Access-Accept or Access-Reject, signed with the same secret.

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
            _ => "message",
        }
    }
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
}
