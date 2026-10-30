//! TACACS+ (RFC 8907) authentication over TCP port 49: a PAP login in one exchange (START,
//! then REPLY), with the body obfuscated by the MD5 pad the shared key makes (§4.5).

use crate::md5::md5;
use crate::{Result, WireError};

pub const TACACS_PORT: u16 = 49;
const VERSION_PAP: u8 = 0xc1;
const TYPE_AUTHEN: u8 = 1;
pub const STATUS_PASS: u8 = 1;
pub const STATUS_FAIL: u8 = 2;

/// A TACACS+ authentication packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tacacs {
    /// A login: user and password (PAP).
    Start { user: String, password: String },
    /// The server's answer.
    Reply { status: u8, message: String },
}

/// The pad: MD5(session id, key, version, seq) chained (RFC 8907 §4.5).
fn pad(session: u32, key: &str, version: u8, seq: u8, len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    while out.len() < len {
        let block = md5(&[&session.to_be_bytes()[..], key.as_bytes(), &[version, seq], &prev].concat());
        out.extend_from_slice(&block);
        prev = block.to_vec();
    }
    out.truncate(len);
    out
}

fn bytes(s: &str) -> &[u8] {
    let b = s.as_bytes();
    b.get(..b.len().min(255)).unwrap_or_default()
}

impl Tacacs {
    pub fn encode(&self, session: u32, seq: u8, key: &str) -> Vec<u8> {
        let body = match self {
            Tacacs::Start { user, password } => {
                let (u, p) = (bytes(user), bytes(password));
                // action login, privilege 1, PAP, service login, then the lengths and fields.
                let mut b = vec![1, 1, 2, 1, u.len() as u8, 0, 0, p.len() as u8];
                b.extend_from_slice(u);
                b.extend_from_slice(p);
                b
            }
            Tacacs::Reply { status, message } => {
                let m = message.as_bytes().get(..message.len().min(1000)).unwrap_or_default();
                let mut b = vec![*status, 0];
                b.extend_from_slice(&u16::try_from(m.len()).unwrap_or(0).to_be_bytes());
                b.extend_from_slice(&[0, 0]);
                b.extend_from_slice(m);
                b
            }
        };
        let p = pad(session, key, VERSION_PAP, seq, body.len());
        let mut out = vec![VERSION_PAP, TYPE_AUTHEN, seq, 0];
        out.extend_from_slice(&session.to_be_bytes());
        out.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
        out.extend(body.iter().zip(p).map(|(a, b)| a ^ b));
        out
    }

    /// The packet, its session id and sequence number, read with the shared key.
    pub fn decode(b: &[u8], key: &str) -> Result<(Tacacs, u32, u8)> {
        let t = || WireError::Truncated("tacacs+");
        let (version, kind, seq) = (*b.first().ok_or_else(t)?, *b.get(1).ok_or_else(t)?, *b.get(2).ok_or_else(t)?);
        if version >> 4 != 0xc || kind != TYPE_AUTHEN {
            return Err(WireError::Unsupported("tacacs+", "not an authentication packet".into()));
        }
        let session = u32::from_be_bytes(b.get(4..8).ok_or_else(t)?.try_into().map_err(|_| t())?);
        let len = u32::from_be_bytes(b.get(8..12).ok_or_else(t)?.try_into().map_err(|_| t())?) as usize;
        let raw = b.get(12..12 + len.min(65_536)).ok_or_else(t)?;
        let body: Vec<u8> = raw.iter().zip(pad(session, key, version, seq, raw.len())).map(|(a, b)| a ^ b).collect();
        let text = |from: usize, n: usize| String::from_utf8_lossy(body.get(from..from + n).unwrap_or_default()).into_owned();
        let packet = if seq % 2 == 1 {
            let (ul, dl) = (usize::from(*body.get(4).ok_or_else(t)?), usize::from(*body.get(7).ok_or_else(t)?));
            let (pl, rl) = (usize::from(*body.get(5).ok_or_else(t)?), usize::from(*body.get(6).ok_or_else(t)?));
            if body.len() < 8 + ul + pl + rl + dl {
                return Err(t());
            }
            Tacacs::Start { user: text(8, ul), password: text(8 + ul + pl + rl, dl) }
        } else {
            let ml = usize::from(u16::from_be_bytes([*body.get(2).ok_or_else(t)?, *body.get(3).ok_or_else(t)?]));
            Tacacs::Reply { status: *body.first().ok_or_else(t)?, message: text(6, ml) }
        };
        Ok((packet, session, seq))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_and_reply_round_trip_only_with_the_key() {
        let s = Tacacs::Start { user: "admin".into(), password: "tac-pass".into() };
        let b = s.encode(0x1234_5678, 1, "k3y");
        assert_eq!(Tacacs::decode(&b, "k3y").unwrap(), (s.clone(), 0x1234_5678, 1));
        assert!(!String::from_utf8_lossy(&b).contains("tac-pass"), "the body is obfuscated");
        assert_ne!(Tacacs::decode(&b, "other").map(|x| x.0).ok(), Some(s));
        let r = Tacacs::Reply { status: STATUS_PASS, message: "Welcome".into() };
        assert_eq!(Tacacs::decode(&r.encode(7, 2, "k3y"), "k3y").unwrap().0, r);
        for k in 0..b.len() {
            let _ = Tacacs::decode(&b[..k], "k3y");
        }
    }
}
