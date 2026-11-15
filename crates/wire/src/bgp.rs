//! BGP-4 messages (RFC 4271): the 19-byte header, OPEN with the four-octet AS number capability
//! (RFC 6793), UPDATE with the path attributes a lab needs (ORIGIN, AS_PATH, NEXT_HOP,
//! MULTI_EXIT_DISC, LOCAL_PREF), NOTIFICATION and KEEPALIVE. Messages travel in TCP segments to
//! or from port 179. AS numbers are always written with four octets in AS_PATH, the way two
//! speakers that announced the capability exchange them.
//!
//! Decoders treat their input as hostile: lengths are checked, counts are bounded.

use std::net::Ipv4Addr;

use netlabcraft_core::Ipv4Net;

use crate::{Result, WireError, be16, ip_at};

/// The TCP port a BGP speaker listens on.
pub const PORT: u16 = 179;
/// The protocol version this encoder writes and reads.
pub const VERSION: u8 = 4;
/// What the two-octet "my AS" field holds for a number that needs four octets (RFC 6793).
pub const AS_TRANS: u16 = 23_456;
/// Marker, length and type.
pub const HEADER_LEN: usize = 19;
/// The longest message (RFC 4271 §4).
pub const MAX_MESSAGE: usize = 4_096;
/// Most prefixes read from one list in an UPDATE (hostile input).
const MAX_PREFIXES: usize = 1_024;
/// Most messages `describe` walks in one segment.
const MAX_MESSAGES: usize = 16;

const TYPE_OPEN: u8 = 1;
const TYPE_UPDATE: u8 = 2;
const TYPE_NOTIFICATION: u8 = 3;
const TYPE_KEEPALIVE: u8 = 4;

/// ORIGIN values.
pub const ORIGIN_IGP: u8 = 0;
pub const ORIGIN_EGP: u8 = 1;
pub const ORIGIN_INCOMPLETE: u8 = 2;

const ATTR_ORIGIN: u8 = 1;
const ATTR_AS_PATH: u8 = 2;
const ATTR_NEXT_HOP: u8 = 3;
const ATTR_MED: u8 = 4;
const ATTR_LOCAL_PREF: u8 = 5;
const FLAG_OPTIONAL: u8 = 0x80;
const FLAG_TRANSITIVE: u8 = 0x40;
const FLAG_EXTENDED: u8 = 0x10;
const SEG_AS_SET: u8 = 1;
const SEG_AS_SEQUENCE: u8 = 2;
const CAP_FOUR_OCTET_AS: u8 = 65;

/// NOTIFICATION error codes (RFC 4271 §4.5).
pub const ERR_OPEN: u8 = 2;
pub const ERR_HOLD_TIMER: u8 = 4;
pub const ERR_CEASE: u8 = 6;
/// OPEN message error subcodes.
pub const OPEN_BAD_PEER_AS: u8 = 2;
/// Cease subcodes (RFC 4486).
pub const CEASE_SHUTDOWN: u8 = 2;
pub const CEASE_RESET: u8 = 4;

/// An OPEN message: the sender's AS number, hold time and BGP identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Open {
    pub asn: u32,
    pub hold_secs: u16,
    pub bgp_id: Ipv4Addr,
}

/// The path attributes of the routes an UPDATE announces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathAttrs {
    pub origin: u8,
    /// The AS_PATH as one sequence, nearest AS first.
    pub as_path: Vec<u32>,
    pub next_hop: Ipv4Addr,
    pub med: Option<u32>,
    pub local_pref: Option<u32>,
}

/// An UPDATE: routes withdrawn, and routes announced with one set of attributes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Update {
    pub withdrawn: Vec<Ipv4Net>,
    pub attrs: Option<PathAttrs>,
    pub nlri: Vec<Ipv4Net>,
}

/// A NOTIFICATION: the error code and subcode (the data field is not used).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notification {
    pub code: u8,
    pub subcode: u8,
}

/// One BGP message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Open(Open),
    Update(Update),
    Notification(Notification),
    Keepalive,
}

fn header(kind: u8, body: &[u8]) -> Vec<u8> {
    let total = u16::try_from(HEADER_LEN + body.len()).unwrap_or(u16::MAX);
    let mut v = vec![0xffu8; 16];
    v.extend_from_slice(&total.to_be_bytes());
    v.push(kind);
    v.extend_from_slice(body);
    v
}

fn attribute(flags: u8, kind: u8, value: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(value.len() + 4);
    if value.len() > 255 {
        v.push(flags | FLAG_EXTENDED);
        v.push(kind);
        v.extend_from_slice(&u16::try_from(value.len()).unwrap_or(u16::MAX).to_be_bytes());
    } else {
        v.push(flags);
        v.push(kind);
        v.push(u8::try_from(value.len()).unwrap_or(u8::MAX));
    }
    v.extend_from_slice(value);
    v
}

fn write_prefixes(out: &mut Vec<u8>, nets: &[Ipv4Net]) {
    for n in nets {
        let n = n.network();
        out.push(n.prefix);
        let bytes = usize::from(n.prefix).div_ceil(8);
        out.extend(n.addr.octets().iter().take(bytes));
    }
}

fn read_prefixes(b: &[u8]) -> Result<Vec<Ipv4Net>> {
    let t = || WireError::Truncated("bgp");
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < b.len() {
        if out.len() >= MAX_PREFIXES {
            return Err(WireError::Unsupported("bgp", "too many prefixes".into()));
        }
        let len = *b.get(at).ok_or_else(t)?;
        if len > 32 {
            return Err(WireError::Unsupported("bgp", format!("prefix length {len}")));
        }
        let n = usize::from(len).div_ceil(8);
        let raw = b.get(at + 1..at + 1 + n).ok_or_else(t)?;
        let mut o = [0u8; 4];
        for (slot, byte) in o.iter_mut().zip(raw) {
            *slot = *byte;
        }
        out.push(Ipv4Net::new(Ipv4Addr::from(o), len).network());
        at += 1 + n;
    }
    Ok(out)
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Message::Keepalive => header(TYPE_KEEPALIVE, &[]),
            Message::Notification(n) => header(TYPE_NOTIFICATION, &[n.code, n.subcode]),
            Message::Open(o) => {
                let mut body = vec![VERSION];
                let two = u16::try_from(o.asn).unwrap_or(AS_TRANS);
                body.extend_from_slice(&two.to_be_bytes());
                body.extend_from_slice(&o.hold_secs.to_be_bytes());
                body.extend_from_slice(&o.bgp_id.octets());
                // One optional parameter: a capability, the four-octet AS number.
                body.push(8);
                body.extend_from_slice(&[2, 6, CAP_FOUR_OCTET_AS, 4]);
                body.extend_from_slice(&o.asn.to_be_bytes());
                header(TYPE_OPEN, &body)
            }
            Message::Update(u) => {
                let mut withdrawn = Vec::new();
                write_prefixes(&mut withdrawn, &u.withdrawn);
                let mut attrs = Vec::new();
                if let Some(a) = &u.attrs {
                    attrs.extend(attribute(FLAG_TRANSITIVE, ATTR_ORIGIN, &[a.origin]));
                    let mut path = Vec::new();
                    if !a.as_path.is_empty() {
                        let list: Vec<u32> = a.as_path.iter().copied().take(255).collect();
                        path.push(SEG_AS_SEQUENCE);
                        path.push(u8::try_from(list.len()).unwrap_or(u8::MAX));
                        for asn in list {
                            path.extend_from_slice(&asn.to_be_bytes());
                        }
                    }
                    attrs.extend(attribute(FLAG_TRANSITIVE, ATTR_AS_PATH, &path));
                    attrs.extend(attribute(FLAG_TRANSITIVE, ATTR_NEXT_HOP, &a.next_hop.octets()));
                    if let Some(m) = a.med {
                        attrs.extend(attribute(FLAG_OPTIONAL, ATTR_MED, &m.to_be_bytes()));
                    }
                    if let Some(l) = a.local_pref {
                        attrs.extend(attribute(FLAG_TRANSITIVE, ATTR_LOCAL_PREF, &l.to_be_bytes()));
                    }
                }
                let mut body = Vec::new();
                body.extend_from_slice(&u16::try_from(withdrawn.len()).unwrap_or(u16::MAX).to_be_bytes());
                body.extend(withdrawn);
                body.extend_from_slice(&u16::try_from(attrs.len()).unwrap_or(u16::MAX).to_be_bytes());
                body.extend(attrs);
                write_prefixes(&mut body, &u.nlri);
                header(TYPE_UPDATE, &body)
            }
        }
    }

    /// Decodes one message; `b` may hold more bytes after it (the length field says where it ends).
    pub fn decode(b: &[u8]) -> Result<Message> {
        let t = || WireError::Truncated("bgp");
        if b.len() < HEADER_LEN {
            return Err(t());
        }
        if b.get(..16).is_none_or(|m| m.iter().any(|x| *x != 0xff)) {
            return Err(WireError::Unsupported("bgp", "bad marker".into()));
        }
        let total = usize::from(be16(b, 16).ok_or_else(t)?);
        if !(HEADER_LEN..=MAX_MESSAGE).contains(&total) {
            return Err(WireError::Unsupported("bgp", format!("length {total}")));
        }
        let kind = *b.get(18).ok_or_else(t)?;
        let body = b.get(HEADER_LEN..total).ok_or_else(t)?;
        match kind {
            TYPE_KEEPALIVE => Ok(Message::Keepalive),
            TYPE_NOTIFICATION => {
                let code = *body.first().ok_or_else(t)?;
                let subcode = *body.get(1).ok_or_else(t)?;
                Ok(Message::Notification(Notification { code, subcode }))
            }
            TYPE_OPEN => {
                let version = *body.first().ok_or_else(t)?;
                if version != VERSION {
                    return Err(WireError::Unsupported("bgp", format!("version {version}")));
                }
                let two = be16(body, 1).ok_or_else(t)?;
                let hold_secs = be16(body, 3).ok_or_else(t)?;
                let bgp_id = ip_at(body, 5).ok_or_else(t)?;
                let opt_len = usize::from(*body.get(9).ok_or_else(t)?);
                let params = body.get(10..10 + opt_len).ok_or_else(t)?;
                let mut asn = u32::from(two);
                let mut at = 0usize;
                // Optional parameters: (type, length, value); type 2 holds capabilities.
                while at + 2 <= params.len() {
                    let ptype = *params.get(at).ok_or_else(t)?;
                    let plen = usize::from(*params.get(at + 1).ok_or_else(t)?);
                    let value = params.get(at + 2..at + 2 + plen).ok_or_else(t)?;
                    if ptype == 2 {
                        let mut c = 0usize;
                        while c + 2 <= value.len() {
                            let code = *value.get(c).ok_or_else(t)?;
                            let clen = usize::from(*value.get(c + 1).ok_or_else(t)?);
                            let cv = value.get(c + 2..c + 2 + clen).ok_or_else(t)?;
                            if code == CAP_FOUR_OCTET_AS && clen == 4 {
                                let raw: [u8; 4] = cv.try_into().map_err(|_| t())?;
                                asn = u32::from_be_bytes(raw);
                            }
                            c += 2 + clen;
                        }
                    }
                    at += 2 + plen;
                }
                Ok(Message::Open(Open { asn, hold_secs, bgp_id }))
            }
            TYPE_UPDATE => {
                let wd_len = usize::from(be16(body, 0).ok_or_else(t)?);
                let withdrawn = read_prefixes(body.get(2..2 + wd_len).ok_or_else(t)?)?;
                let attr_at = 2 + wd_len;
                let attr_len = usize::from(be16(body, attr_at).ok_or_else(t)?);
                let attr_bytes = body.get(attr_at + 2..attr_at + 2 + attr_len).ok_or_else(t)?;
                let nlri = read_prefixes(body.get(attr_at + 2 + attr_len..).ok_or_else(t)?)?;
                let attrs = if attr_len == 0 { None } else { Some(read_attrs(attr_bytes)?) };
                Ok(Message::Update(Update { withdrawn, attrs, nlri }))
            }
            other => Err(WireError::Unsupported("bgp", format!("message type {other}"))),
        }
    }
}

fn read_attrs(b: &[u8]) -> Result<PathAttrs> {
    let t = || WireError::Truncated("bgp");
    let mut a = PathAttrs { origin: ORIGIN_INCOMPLETE, as_path: Vec::new(), next_hop: Ipv4Addr::UNSPECIFIED, med: None, local_pref: None };
    let mut at = 0usize;
    // Every attribute takes at least three bytes, so this loop is bounded by the input.
    while at < b.len() {
        let flags = *b.get(at).ok_or_else(t)?;
        let kind = *b.get(at + 1).ok_or_else(t)?;
        let (len, head) = if flags & FLAG_EXTENDED != 0 {
            (usize::from(be16(b, at + 2).ok_or_else(t)?), 4)
        } else {
            (usize::from(*b.get(at + 2).ok_or_else(t)?), 3)
        };
        let value = b.get(at + head..at + head + len).ok_or_else(t)?;
        match kind {
            ATTR_ORIGIN => a.origin = *value.first().ok_or_else(t)?,
            ATTR_AS_PATH => {
                let mut s = 0usize;
                while s + 2 <= value.len() {
                    let seg = *value.get(s).ok_or_else(t)?;
                    let n = usize::from(*value.get(s + 1).ok_or_else(t)?);
                    let asns = value.get(s + 2..s + 2 + n * 4).ok_or_else(t)?;
                    if seg == SEG_AS_SEQUENCE || seg == SEG_AS_SET {
                        for c in asns.chunks_exact(4) {
                            let raw: [u8; 4] = c.try_into().map_err(|_| t())?;
                            a.as_path.push(u32::from_be_bytes(raw));
                        }
                    }
                    s += 2 + n * 4;
                }
            }
            ATTR_NEXT_HOP => a.next_hop = ip_at(value, 0).ok_or_else(t)?,
            ATTR_MED => {
                let raw: [u8; 4] = value.get(..4).ok_or_else(t)?.try_into().map_err(|_| t())?;
                a.med = Some(u32::from_be_bytes(raw));
            }
            ATTR_LOCAL_PREF => {
                let raw: [u8; 4] = value.get(..4).ok_or_else(t)?.try_into().map_err(|_| t())?;
                a.local_pref = Some(u32::from_be_bytes(raw));
            }
            _ => {}
        }
        at += head + len;
    }
    Ok(a)
}

/// What a NOTIFICATION's code and subcode mean, in words.
pub fn notification_text(code: u8, subcode: u8) -> String {
    match (code, subcode) {
        (ERR_OPEN, OPEN_BAD_PEER_AS) => "OPEN message error: bad peer AS".into(),
        (ERR_OPEN, _) => "OPEN message error".into(),
        (ERR_HOLD_TIMER, _) => "hold timer expired".into(),
        (ERR_CEASE, CEASE_SHUTDOWN) => "cease: neighbour administratively shut down".into(),
        (ERR_CEASE, CEASE_RESET) => "cease: administrative reset".into(),
        (ERR_CEASE, _) => "cease".into(),
        (c, s) => format!("error {c}/{s}"),
    }
}

/// A short description of a message.
pub fn message_text(m: &Message) -> String {
    match m {
        Message::Keepalive => "KEEPALIVE".into(),
        Message::Open(o) => format!("OPEN (AS {}, hold time {} s, id {})", o.asn, o.hold_secs, o.bgp_id),
        Message::Update(u) => format!("UPDATE ({} announced, {} withdrawn)", u.nlri.len(), u.withdrawn.len()),
        Message::Notification(n) => format!("NOTIFICATION ({})", notification_text(n.code, n.subcode)),
    }
}

/// Every message a TCP segment's data holds, one after another; reading stops at the first one
/// that does not decode.
pub fn decode_all(payload: &[u8]) -> Vec<Message> {
    let mut out = Vec::new();
    let mut at = 0usize;
    for _ in 0..MAX_MESSAGES {
        let Some(rest) = payload.get(at..).filter(|r| !r.is_empty()) else { break };
        let Some(len) = be16(rest, 16).map(usize::from).filter(|l| *l >= HEADER_LEN) else { break };
        match Message::decode(rest) {
            Ok(m) => out.push(m),
            Err(_) => break,
        }
        at += len;
    }
    out
}

/// A description of what a TCP segment's data holds: its messages, one after another.
pub fn describe(payload: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut at = 0usize;
    for _ in 0..MAX_MESSAGES {
        let Some(rest) = payload.get(at..).filter(|r| !r.is_empty()) else { break };
        let Some(len) = be16(rest, 16).map(usize::from).filter(|l| *l >= HEADER_LEN) else {
            parts.push("message (truncated)".to_string());
            break;
        };
        match Message::decode(rest) {
            Ok(m) => parts.push(message_text(&m)),
            Err(e) => parts.push(format!("message ({e})")),
        }
        at += len;
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(s: &str) -> Ipv4Net {
        s.parse().unwrap()
    }

    #[test]
    fn open_carries_a_four_octet_as_number() {
        let m = Message::Open(Open { asn: 4_200_000_001, hold_secs: 180, bgp_id: Ipv4Addr::new(1, 1, 1, 1) });
        let b = m.encode();
        // The two-octet field holds AS_TRANS; the capability holds the real number.
        assert_eq!(be16(&b, 20), Some(AS_TRANS));
        assert_eq!(Message::decode(&b), Ok(m));
        let small = Message::Open(Open { asn: 65_001, hold_secs: 90, bgp_id: Ipv4Addr::new(2, 2, 2, 2) });
        assert_eq!(Message::decode(&small.encode()), Ok(small));
    }

    #[test]
    fn keepalive_and_notification_round_trip() {
        assert_eq!(Message::Keepalive.encode().len(), HEADER_LEN);
        assert_eq!(Message::decode(&Message::Keepalive.encode()), Ok(Message::Keepalive));
        let n = Message::Notification(Notification { code: ERR_OPEN, subcode: OPEN_BAD_PEER_AS });
        assert_eq!(Message::decode(&n.encode()), Ok(n));
        assert_eq!(notification_text(ERR_OPEN, OPEN_BAD_PEER_AS), "OPEN message error: bad peer AS");
    }

    #[test]
    fn update_round_trips_attributes_and_prefixes() {
        let u = Message::Update(Update {
            withdrawn: vec![net("172.16.0.0/12")],
            attrs: Some(PathAttrs {
                origin: ORIGIN_IGP,
                as_path: vec![65_002, 65_003, 4_200_000_001],
                next_hop: Ipv4Addr::new(10, 0, 12, 2),
                med: Some(20),
                local_pref: Some(150),
            }),
            nlri: vec![net("10.2.0.0/16"), net("192.168.5.0/24"), net("0.0.0.0/0"), net("203.0.113.7/32")],
        });
        assert_eq!(Message::decode(&u.encode()), Ok(u.clone()));
        assert_eq!(message_text(&u), "UPDATE (4 announced, 1 withdrawn)");
        // A withdrawal alone carries no attributes.
        let w = Message::Update(Update { withdrawn: vec![net("10.2.0.0/16")], attrs: None, nlri: Vec::new() });
        assert_eq!(Message::decode(&w.encode()), Ok(w));
    }

    #[test]
    fn a_long_as_path_uses_the_extended_length() {
        let path: Vec<u32> = (1..=200).collect();
        let u = Message::Update(Update {
            withdrawn: Vec::new(),
            attrs: Some(PathAttrs { origin: ORIGIN_EGP, as_path: path, next_hop: Ipv4Addr::new(10, 0, 0, 1), med: None, local_pref: None }),
            nlri: vec![net("10.9.0.0/16")],
        });
        assert_eq!(Message::decode(&u.encode()), Ok(u));
    }

    #[test]
    fn decoders_survive_every_truncation_and_garbage() {
        let messages = [
            Message::Keepalive,
            Message::Open(Open { asn: 65_001, hold_secs: 180, bgp_id: Ipv4Addr::new(1, 1, 1, 1) }),
            Message::Notification(Notification { code: ERR_CEASE, subcode: CEASE_RESET }),
            Message::Update(Update {
                withdrawn: vec![net("10.1.0.0/16")],
                attrs: Some(PathAttrs {
                    origin: ORIGIN_INCOMPLETE,
                    as_path: vec![65_001],
                    next_hop: Ipv4Addr::new(10, 0, 0, 1),
                    med: Some(1),
                    local_pref: Some(100),
                }),
                nlri: vec![net("10.2.0.0/16")],
            }),
        ];
        for m in &messages {
            let b = m.encode();
            for n in 0..b.len() {
                let cut = b.get(..n).unwrap_or_default();
                assert!(Message::decode(cut).is_err(), "{n} bytes of {m:?}");
                let _ = describe(cut);
            }
            // Flip each byte after the marker: never a panic.
            for i in 16..b.len() {
                let mut x = b.clone();
                if let Some(v) = x.get_mut(i) {
                    *v = !*v;
                }
                let _ = Message::decode(&x);
                let _ = describe(&x);
            }
        }
        assert!(Message::decode(&[0u8; 40]).is_err());
        let mut bad_len = Message::Keepalive.encode();
        bad_len[16] = 0xff;
        bad_len[17] = 0xff;
        assert!(Message::decode(&bad_len).is_err());
    }

    #[test]
    fn describe_walks_several_messages_in_one_segment() {
        let mut data = Message::Keepalive.encode();
        data.extend(Message::Open(Open { asn: 65_001, hold_secs: 180, bgp_id: Ipv4Addr::new(1, 1, 1, 1) }).encode());
        assert_eq!(describe(&data), "KEEPALIVE, OPEN (AS 65001, hold time 180 s, id 1.1.1.1)");
    }
}
