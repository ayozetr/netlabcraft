//! TACACS+ (RFC 8907) over TCP port 49: authentication (a PAP login in one exchange, START then
//! REPLY), authorization (§6: the arguments a user may have, such as `priv-lvl`, or a command)
//! and accounting (§7: start, stop and command records), with the body obfuscated by the MD5 pad
//! the shared key makes (§4.5).

use crate::md5::md5;
use crate::{Result, WireError};

pub const TACACS_PORT: u16 = 49;
const VERSION_PAP: u8 = 0xc1;
const TYPE_AUTHEN: u8 = 1;
const TYPE_AUTHOR: u8 = 2;
const TYPE_ACCT: u8 = 3;
pub const STATUS_PASS: u8 = 1;
pub const STATUS_FAIL: u8 = 2;
/// Authorization: the server adds the arguments it sent to what was asked.
pub const AUTHOR_PASS_ADD: u8 = 1;
/// Authorization: refused.
pub const AUTHOR_FAIL: u8 = 0x10;
/// Accounting: the record was kept.
pub const ACCT_SUCCESS: u8 = 1;
/// Accounting flag: the start of a session or task.
pub const ACCT_FLAG_START: u8 = 0x02;
/// Accounting flag: the end of a session or task.
pub const ACCT_FLAG_STOP: u8 = 0x04;
/// Most arguments a packet carries.
const MAX_ARGS: usize = 32;

/// A TACACS+ authentication packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tacacs {
    /// A login: user and password (PAP).
    Start { user: String, password: String },
    /// The server's answer.
    Reply { status: u8, message: String },
    /// Authorization request: the user, the privilege level it is at and the arguments
    /// (`service=shell`, `cmd=show`, `cmd-arg=version`).
    AuthorRequest { user: String, privilege: u8, args: Vec<String> },
    /// Authorization answer: a status and the arguments the server grants (`priv-lvl=15`).
    AuthorReply { status: u8, args: Vec<String> },
    /// Accounting record: the flags (start, stop), the user, its privilege level and the arguments.
    AcctRequest { flags: u8, user: String, privilege: u8, args: Vec<String> },
    /// Accounting answer.
    AcctReply { status: u8 },
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

/// At most [`MAX_ARGS`] arguments of at most 255 bytes each.
fn clip(args: &[String]) -> Vec<Vec<u8>> {
    args.iter().take(MAX_ARGS).map(|a| bytes(a).to_vec()).collect()
}

/// An authorization or accounting request body: the fixed bytes of the kind (authorization:
/// method, privilege, type, service; accounting: flags first), the lengths, then the fields.
fn request_body(fixed: &[u8], user: &str, args: &[String]) -> Vec<u8> {
    let args = clip(args);
    let u = bytes(user);
    let mut b = fixed.to_vec();
    b.extend_from_slice(&[u.len() as u8, 0, 0, args.len() as u8]);
    b.extend(args.iter().map(|a| a.len() as u8));
    b.extend_from_slice(u);
    for a in &args {
        b.extend_from_slice(a);
    }
    b
}

fn bytes(s: &str) -> &[u8] {
    let b = s.as_bytes();
    b.get(..b.len().min(255)).unwrap_or_default()
}

impl Tacacs {
    pub fn encode(&self, session: u32, seq: u8, key: &str) -> Vec<u8> {
        let kind = match self {
            Tacacs::Start { .. } | Tacacs::Reply { .. } => TYPE_AUTHEN,
            Tacacs::AuthorRequest { .. } | Tacacs::AuthorReply { .. } => TYPE_AUTHOR,
            Tacacs::AcctRequest { .. } | Tacacs::AcctReply { .. } => TYPE_ACCT,
        };
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
            Tacacs::AuthorRequest { user, privilege, args } => request_body(&[1, *privilege, 1, 1], user, args),
            Tacacs::AcctRequest { flags, user, privilege, args } => request_body(&[*flags, 1, *privilege, 1, 1], user, args),
            Tacacs::AuthorReply { status, args } => {
                let args = clip(args);
                let mut b = vec![*status, args.len() as u8, 0, 0, 0, 0];
                b.extend(args.iter().map(|a| a.len() as u8));
                for a in &args {
                    b.extend_from_slice(a);
                }
                b
            }
            Tacacs::AcctReply { status } => vec![0, 0, 0, 0, *status],
        };
        let p = pad(session, key, VERSION_PAP, seq, body.len());
        let mut out = vec![VERSION_PAP, kind, seq, 0];
        out.extend_from_slice(&session.to_be_bytes());
        out.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
        out.extend(body.iter().zip(p).map(|(a, b)| a ^ b));
        out
    }

    /// The packet, its session id and sequence number, read with the shared key.
    pub fn decode(b: &[u8], key: &str) -> Result<(Tacacs, u32, u8)> {
        let t = || WireError::Truncated("tacacs+");
        let (version, kind, seq) = (*b.first().ok_or_else(t)?, *b.get(1).ok_or_else(t)?, *b.get(2).ok_or_else(t)?);
        if version >> 4 != 0xc || !matches!(kind, TYPE_AUTHEN | TYPE_AUTHOR | TYPE_ACCT) {
            return Err(WireError::Unsupported("tacacs+", "not an authentication, authorization or accounting packet".into()));
        }
        let session = u32::from_be_bytes(b.get(4..8).ok_or_else(t)?.try_into().map_err(|_| t())?);
        let len = u32::from_be_bytes(b.get(8..12).ok_or_else(t)?.try_into().map_err(|_| t())?) as usize;
        let raw = b.get(12..12 + len.min(65_536)).ok_or_else(t)?;
        let body: Vec<u8> = raw.iter().zip(pad(session, key, version, seq, raw.len())).map(|(a, b)| a ^ b).collect();
        let text = |from: usize, n: usize| String::from_utf8_lossy(body.get(from..from + n).unwrap_or_default()).into_owned();
        let packet = if kind == TYPE_AUTHOR || kind == TYPE_ACCT {
            decode_body(kind, seq % 2 == 1, &body).ok_or_else(t)?
        } else if seq % 2 == 1 {
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

/// The arguments of a request or an answer: `count` lengths at `at`, then the fields from `from`.
fn fields(body: &[u8], count: usize, at: usize, from: usize) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut pos = from;
    for i in 0..count.min(MAX_ARGS) {
        let n = usize::from(*body.get(at + i)?);
        out.push(String::from_utf8_lossy(body.get(pos..pos + n)?).into_owned());
        pos += n;
    }
    Some(out)
}

fn decode_body(kind: u8, request: bool, body: &[u8]) -> Option<Tacacs> {
    match (kind, request) {
        (TYPE_AUTHOR, true) | (TYPE_ACCT, true) => {
            // Authorization: 4 fixed bytes; accounting: 5. Then user, port and address lengths and the count.
            let fixed = if kind == TYPE_AUTHOR { 4 } else { 5 };
            let (ul, pl, rl, count) = (
                usize::from(*body.get(fixed)?),
                usize::from(*body.get(fixed + 1)?),
                usize::from(*body.get(fixed + 2)?),
                usize::from(*body.get(fixed + 3)?),
            );
            let lens = fixed + 4;
            let user_at = lens + count.min(MAX_ARGS);
            let user = String::from_utf8_lossy(body.get(user_at..user_at + ul)?).into_owned();
            let args = fields(body, count, lens, user_at + ul + pl + rl)?;
            Some(if kind == TYPE_AUTHOR {
                Tacacs::AuthorRequest { user, privilege: *body.get(1)?, args }
            } else {
                Tacacs::AcctRequest { flags: *body.first()?, user, privilege: *body.get(2)?, args }
            })
        }
        (TYPE_AUTHOR, false) => {
            let count = usize::from(*body.get(1)?);
            let (ml, dl) =
                (usize::from(u16::from_be_bytes([*body.get(2)?, *body.get(3)?])), usize::from(u16::from_be_bytes([*body.get(4)?, *body.get(5)?])));
            let lens = 6;
            let args = fields(body, count, lens, lens + count.min(MAX_ARGS) + ml + dl)?;
            Some(Tacacs::AuthorReply { status: *body.first()?, args })
        }
        (TYPE_ACCT, false) => Some(Tacacs::AcctReply { status: *body.get(4)? }),
        _ => None,
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

    #[test]
    fn authorization_and_accounting_round_trip_and_survive_truncation() {
        let packets = [
            Tacacs::AuthorRequest { user: "ana".into(), privilege: 1, args: vec!["service=shell".into(), "cmd=".into()] },
            Tacacs::AuthorReply { status: AUTHOR_PASS_ADD, args: vec!["priv-lvl=7".into()] },
            Tacacs::AuthorReply { status: AUTHOR_FAIL, args: vec![] },
            Tacacs::AcctRequest {
                flags: ACCT_FLAG_START,
                user: "ana".into(),
                privilege: 15,
                args: vec!["task_id=3".into(), "service=shell".into(), "cmd=show version".into()],
            },
            Tacacs::AcctReply { status: ACCT_SUCCESS },
        ];
        for (i, p) in packets.iter().enumerate() {
            let seq = if matches!(p, Tacacs::AuthorRequest { .. } | Tacacs::AcctRequest { .. }) { 1 } else { 2 };
            let b = p.encode(77 + i as u32, seq, "k3y");
            let (back, session, s) = Tacacs::decode(&b, "k3y").unwrap();
            assert_eq!((&back, session, s), (p, 77 + i as u32, seq));
            assert_ne!(Tacacs::decode(&b, "other").map(|x| x.0).ok().as_ref(), Some(p));
            for k in 0..b.len() {
                let _ = Tacacs::decode(&b[..k], "k3y");
            }
        }
    }
}
