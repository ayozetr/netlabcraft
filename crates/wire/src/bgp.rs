//! BGP-4 messages (RFC 4271): the 19-byte header, OPEN with the four-octet AS number capability
//! (RFC 6793), UPDATE with the path attributes a lab needs (ORIGIN, AS_PATH, NEXT_HOP,
//! MULTI_EXIT_DISC, LOCAL_PREF, COMMUNITIES (RFC 1997), the AS_SET segment an aggregate
//! carries, ORIGINATOR_ID and CLUSTER_LIST (RFC 4456, route reflection) and the AS_CONFED_SEQUENCE
//! and AS_CONFED_SET segments of a confederation (RFC 5065)), NOTIFICATION and KEEPALIVE. Messages travel in TCP segments to
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
const ATTR_COMMUNITIES: u8 = 8;
const ATTR_ORIGINATOR_ID: u8 = 9;
const ATTR_CLUSTER_LIST: u8 = 10;
const ATTR_EXT_COMMUNITIES: u8 = 16;
/// Most extended communities read from, or written to, one attribute (hostile input).
pub const MAX_EXT_COMMUNITIES: usize = 32;
/// Most communities read from one attribute (hostile input).
const MAX_COMMUNITIES: usize = 64;
/// Most cluster ids read from, or written to, one CLUSTER_LIST (hostile input).
pub const MAX_CLUSTERS: usize = 32;
const FLAG_OPTIONAL: u8 = 0x80;
const FLAG_TRANSITIVE: u8 = 0x40;
const FLAG_EXTENDED: u8 = 0x10;
const SEG_AS_SET: u8 = 1;
const SEG_AS_SEQUENCE: u8 = 2;
const SEG_CONFED_SEQUENCE: u8 = 3;
const SEG_CONFED_SET: u8 = 4;
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

/// Extended community types (RFC 4360, the high-order octet without its two flag bits) and subtypes.
pub const EXT_TWO_OCTET_AS: u8 = 0x00;
pub const EXT_IPV4_ADDRESS: u8 = 0x01;
/// RFC 5668: the global administrator is a four-octet AS number.
pub const EXT_FOUR_OCTET_AS: u8 = 0x02;
pub const EXT_SUB_ROUTE_TARGET: u8 = 0x02;
pub const EXT_SUB_ROUTE_ORIGIN: u8 = 0x03;
/// The bit of the type octet that marks a community as non-transitive.
const EXT_NON_TRANSITIVE: u8 = 0x40;

/// What an extended community says about a route: the VPN a route belongs to (a route target) or
/// the site it came from (a route origin, also called site of origin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtKind {
    RouteTarget,
    RouteOrigin,
}

impl ExtKind {
    /// The word the console uses: `rt` or `soo`.
    pub fn word(self) -> &'static str {
        match self {
            ExtKind::RouteTarget => "rt",
            ExtKind::RouteOrigin => "soo",
        }
    }

    fn label(self) -> &'static str {
        match self {
            ExtKind::RouteTarget => "RT",
            ExtKind::RouteOrigin => "SoO",
        }
    }

    fn subtype(self) -> u8 {
        match self {
            ExtKind::RouteTarget => EXT_SUB_ROUTE_TARGET,
            ExtKind::RouteOrigin => EXT_SUB_ROUTE_ORIGIN,
        }
    }
}

/// An extended community as eight octets in one number (RFC 4360): the type, the subtype and six
/// octets of value (a global administrator and a local one). Built from `kind` and the two parts.
fn ext_build(kind_type: u8, kind: ExtKind, global: &[u8], local: &[u8]) -> u64 {
    let mut b = [0u8; 8];
    for (slot, v) in b.iter_mut().zip([kind_type, kind.subtype()].iter().chain(global).chain(local)) {
        *slot = *v;
    }
    u64::from_be_bytes(b)
}

/// A route target or route origin with a two-octet AS number and a four-octet local value.
pub fn ext_as2(kind: ExtKind, asn: u16, local: u32) -> u64 {
    ext_build(EXT_TWO_OCTET_AS, kind, &asn.to_be_bytes(), &local.to_be_bytes())
}

/// One with an IPv4 address and a two-octet local value.
pub fn ext_ipv4(kind: ExtKind, addr: Ipv4Addr, local: u16) -> u64 {
    ext_build(EXT_IPV4_ADDRESS, kind, &addr.octets(), &local.to_be_bytes())
}

/// One with a four-octet AS number and a two-octet local value (RFC 5668).
pub fn ext_as4(kind: ExtKind, asn: u32, local: u16) -> u64 {
    ext_build(EXT_FOUR_OCTET_AS, kind, &asn.to_be_bytes(), &local.to_be_bytes())
}

/// `ASN:NN` or `A.B.C.D:NN`, as the console writes a route target or origin. An AS number that
/// fits in two octets gets the two-octet form (and a 32-bit local value), a larger one the
/// four-octet form (and a 16-bit local value), an address the IPv4 form.
pub fn ext_parse(kind: ExtKind, text: &str) -> Option<u64> {
    let (global, local) = text.trim().rsplit_once(':')?;
    if global.contains('.') {
        let addr: Ipv4Addr = global.parse().ok()?;
        return Some(ext_ipv4(kind, addr, local.parse().ok()?));
    }
    let asn: u32 = global.parse().ok()?;
    match u16::try_from(asn) {
        Ok(small) => Some(ext_as2(kind, small, local.parse().ok()?)),
        Err(_) => Some(ext_as4(kind, asn, local.parse().ok()?)),
    }
}

/// Whether an extended community is a route target or a route origin, and which.
pub fn ext_kind(c: u64) -> Option<ExtKind> {
    let b = c.to_be_bytes();
    let (kind_type, sub) = (b.first().copied()? & !EXT_NON_TRANSITIVE & 0x3f, b.get(1).copied()?);
    if !matches!(kind_type, EXT_TWO_OCTET_AS | EXT_IPV4_ADDRESS | EXT_FOUR_OCTET_AS) {
        return None;
    }
    match sub {
        EXT_SUB_ROUTE_TARGET => Some(ExtKind::RouteTarget),
        EXT_SUB_ROUTE_ORIGIN => Some(ExtKind::RouteOrigin),
        _ => None,
    }
}

/// Whether the community may cross an AS boundary (the non-transitive bit is clear).
pub fn ext_transitive(c: u64) -> bool {
    c.to_be_bytes().first().is_some_and(|t| t & EXT_NON_TRANSITIVE == 0)
}

/// An extended community as the console shows it: `RT:65001:100`, `SoO:10.0.0.1:5`, or the eight
/// octets in hex for a type this simulator does not name.
pub fn ext_text(c: u64) -> String {
    let b = c.to_be_bytes();
    let (Some(kind), Some(t)) = (ext_kind(c), b.first().map(|t| t & 0x3f)) else { return format!("0x{c:016x}") };
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let label = kind.label();
    match t {
        EXT_TWO_OCTET_AS => {
            format!("{label}:{}:{}", u16::from_be_bytes([at(2), at(3)]), u32::from_be_bytes([at(4), at(5), at(6), at(7)]))
        }
        EXT_IPV4_ADDRESS => format!("{label}:{}:{}", Ipv4Addr::new(at(2), at(3), at(4), at(5)), u16::from_be_bytes([at(6), at(7)])),
        _ => format!("{label}:{}:{}", u32::from_be_bytes([at(2), at(3), at(4), at(5)]), u16::from_be_bytes([at(6), at(7)])),
    }
}

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
    /// The AS numbers of an AS_SET segment after the sequence (what an aggregate with `as-set`
    /// carries); it counts as one hop in the path length.
    pub as_set: Vec<u32>,
    /// The COMMUNITIES attribute (RFC 1997), each community as one 32-bit value.
    pub communities: Vec<u32>,
    /// ORIGINATOR_ID (RFC 4456): the router id of the speaker that first told the AS about the
    /// route, set by the first route reflector. Never leaves the AS.
    pub originator_id: Option<Ipv4Addr>,
    /// CLUSTER_LIST (RFC 4456): the clusters a reflected route went through, the latest first.
    pub cluster_list: Vec<Ipv4Addr>,
    /// The AS_CONFED_SEQUENCE segment at the front of the AS_PATH (RFC 5065): the member
    /// autonomous systems of a confederation the route crossed, nearest first.
    pub confed_path: Vec<u32>,
    /// The AS_CONFED_SET segment (RFC 5065).
    pub confed_set: Vec<u32>,
    /// The EXTENDED COMMUNITIES attribute (RFC 4360), each as eight octets in one number (see
    /// [`ext_as2`], [`ext_ipv4`] and [`ext_text`]).
    pub ext_communities: Vec<u64>,
}

impl Default for PathAttrs {
    /// An IGP origin, an empty path, an unspecified next hop and nothing optional: what a test or a
    /// caller overrides field by field.
    fn default() -> PathAttrs {
        PathAttrs {
            origin: ORIGIN_IGP,
            as_path: Vec::new(),
            next_hop: Ipv4Addr::UNSPECIFIED,
            med: None,
            local_pref: None,
            as_set: Vec::new(),
            communities: Vec::new(),
            originator_id: None,
            cluster_list: Vec::new(),
            confed_path: Vec::new(),
            confed_set: Vec::new(),
            ext_communities: Vec::new(),
        }
    }
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
// An UPDATE carries the path attributes inline; boxing them would change every constructor for no gain, since
// messages are built and dropped one at a time.
#[allow(clippy::large_enum_variant)]
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
                    // The confederation segments come first, as RFC 5065 puts them.
                    for (kind, members) in [
                        (SEG_CONFED_SEQUENCE, &a.confed_path),
                        (SEG_CONFED_SET, &a.confed_set),
                        (SEG_AS_SEQUENCE, &a.as_path),
                        (SEG_AS_SET, &a.as_set),
                    ] {
                        if !members.is_empty() {
                            let list: Vec<u32> = members.iter().copied().take(255).collect();
                            path.push(kind);
                            path.push(u8::try_from(list.len()).unwrap_or(u8::MAX));
                            for asn in list {
                                path.extend_from_slice(&asn.to_be_bytes());
                            }
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
                    if !a.communities.is_empty() {
                        let value: Vec<u8> = a.communities.iter().take(MAX_COMMUNITIES).flat_map(|c| c.to_be_bytes()).collect();
                        attrs.extend(attribute(FLAG_OPTIONAL | FLAG_TRANSITIVE, ATTR_COMMUNITIES, &value));
                    }
                    if !a.ext_communities.is_empty() {
                        let value: Vec<u8> = a.ext_communities.iter().take(MAX_EXT_COMMUNITIES).flat_map(|c| c.to_be_bytes()).collect();
                        attrs.extend(attribute(FLAG_OPTIONAL | FLAG_TRANSITIVE, ATTR_EXT_COMMUNITIES, &value));
                    }
                    if let Some(o) = a.originator_id {
                        attrs.extend(attribute(FLAG_OPTIONAL, ATTR_ORIGINATOR_ID, &o.octets()));
                    }
                    if !a.cluster_list.is_empty() {
                        let value: Vec<u8> = a.cluster_list.iter().take(MAX_CLUSTERS).flat_map(|c| c.octets()).collect();
                        attrs.extend(attribute(FLAG_OPTIONAL, ATTR_CLUSTER_LIST, &value));
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
    let mut a = PathAttrs {
        origin: ORIGIN_INCOMPLETE,
        as_path: Vec::new(),
        next_hop: Ipv4Addr::UNSPECIFIED,
        med: None,
        local_pref: None,
        as_set: Vec::new(),
        communities: Vec::new(),
        originator_id: None,
        cluster_list: Vec::new(),
        confed_path: Vec::new(),
        confed_set: Vec::new(),
        ext_communities: Vec::new(),
    };
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
                    if matches!(seg, SEG_AS_SEQUENCE | SEG_AS_SET | SEG_CONFED_SEQUENCE | SEG_CONFED_SET) {
                        for c in asns.chunks_exact(4) {
                            let raw: [u8; 4] = c.try_into().map_err(|_| t())?;
                            let asn = u32::from_be_bytes(raw);
                            match seg {
                                SEG_AS_SET => a.as_set.push(asn),
                                SEG_CONFED_SEQUENCE => a.confed_path.push(asn),
                                SEG_CONFED_SET => a.confed_set.push(asn),
                                _ => a.as_path.push(asn),
                            }
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
            ATTR_COMMUNITIES => {
                for c in value.chunks_exact(4).take(MAX_COMMUNITIES) {
                    let raw: [u8; 4] = c.try_into().map_err(|_| t())?;
                    a.communities.push(u32::from_be_bytes(raw));
                }
            }
            ATTR_EXT_COMMUNITIES => {
                for c in value.chunks_exact(8).take(MAX_EXT_COMMUNITIES) {
                    let raw: [u8; 8] = c.try_into().map_err(|_| t())?;
                    a.ext_communities.push(u64::from_be_bytes(raw));
                }
            }
            ATTR_ORIGINATOR_ID => a.originator_id = Some(ip_at(value, 0).ok_or_else(t)?),
            ATTR_CLUSTER_LIST => {
                for c in value.chunks_exact(4).take(MAX_CLUSTERS) {
                    let raw: [u8; 4] = c.try_into().map_err(|_| t())?;
                    a.cluster_list.push(Ipv4Addr::from(raw));
                }
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
                as_set: vec![65_010, 65_011],
                communities: vec![0x0001_0064, 0xFFFF_FF01],
                originator_id: Some(Ipv4Addr::new(1, 1, 1, 1)),
                cluster_list: vec![Ipv4Addr::new(2, 2, 2, 2), Ipv4Addr::new(3, 3, 3, 3)],
                confed_path: vec![65_010, 65_011],
                confed_set: vec![65_012],
                ext_communities: vec![ext_as2(ExtKind::RouteTarget, 65_001, 100), ext_ipv4(ExtKind::RouteOrigin, Ipv4Addr::new(10, 0, 0, 1), 5)],
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
            attrs: Some(PathAttrs {
                origin: ORIGIN_EGP,
                as_path: path,
                next_hop: Ipv4Addr::new(10, 0, 0, 1),
                med: None,
                local_pref: None,
                as_set: Vec::new(),
                communities: Vec::new(),
                ..PathAttrs::default()
            }),
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
                    as_set: vec![65_002],
                    communities: vec![1, 2, 3],
                    originator_id: Some(Ipv4Addr::new(9, 9, 9, 9)),
                    cluster_list: vec![Ipv4Addr::new(8, 8, 8, 8)],
                    confed_path: vec![65_003],
                    confed_set: vec![65_004],
                    ext_communities: vec![ext_as2(ExtKind::RouteTarget, 65_001, 100), ext_as4(ExtKind::RouteOrigin, 4_200_000_001, 7)],
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
    fn communities_and_an_as_set_survive_the_wire_and_an_update_without_them_decodes_empty() {
        let attrs = PathAttrs {
            origin: ORIGIN_IGP,
            as_path: vec![65_001],
            next_hop: Ipv4Addr::new(10, 0, 0, 1),
            med: None,
            local_pref: None,
            as_set: vec![65_005, 65_006],
            communities: vec![0xFFFF_FF02, 0x0064_00C8],
            ..PathAttrs::default()
        };
        let u = Message::Update(Update { withdrawn: Vec::new(), attrs: Some(attrs.clone()), nlri: vec![net("10.0.0.0/8")] });
        let bytes = u.encode();
        let Ok(Message::Update(back)) = Message::decode(&bytes) else { panic!("did not decode") };
        assert_eq!(back.attrs, Some(attrs.clone()));
        // The attribute is optional and transitive (flags 0xC0), type 8, and four octets per community.
        let at = bytes.windows(3).position(|w| w == [0xC0, ATTR_COMMUNITIES, 8]);
        assert!(at.is_some(), "{bytes:?}");
        let plain = Message::Update(Update {
            withdrawn: Vec::new(),
            attrs: Some(PathAttrs { as_set: Vec::new(), communities: Vec::new(), ..attrs }),
            nlri: vec![net("10.0.0.0/8")],
        });
        let Ok(Message::Update(back)) = Message::decode(&plain.encode()) else { panic!("did not decode") };
        assert!(back.attrs.is_some_and(|a| a.communities.is_empty() && a.as_set.is_empty()));
    }

    #[test]
    fn extended_communities_use_the_rfc_4360_layout() {
        // Two-octet AS: type 0x00, subtype 0x02 (route target), the AS, then a four-octet value.
        let rt = ext_as2(ExtKind::RouteTarget, 65_001, 100);
        assert_eq!(rt.to_be_bytes(), [0x00, 0x02, 0xFD, 0xE9, 0, 0, 0, 100]);
        // IPv4 address: type 0x01, subtype 0x03 (route origin), the address, then a two-octet value.
        let soo = ext_ipv4(ExtKind::RouteOrigin, Ipv4Addr::new(10, 0, 0, 1), 5);
        assert_eq!(soo.to_be_bytes(), [0x01, 0x03, 10, 0, 0, 1, 0, 5]);
        // Four-octet AS (RFC 5668): type 0x02.
        let big = ext_as4(ExtKind::RouteTarget, 4_200_000_001, 7);
        assert_eq!(big.to_be_bytes(), [0x02, 0x02, 0xFA, 0x56, 0xEA, 0x01, 0, 7]);
        assert_eq!((ext_text(rt), ext_text(soo), ext_text(big)), ("RT:65001:100".into(), "SoO:10.0.0.1:5".into(), "RT:4200000001:7".into()));
        assert_eq!((ext_kind(rt), ext_kind(soo), ext_kind(0x0005_0000_0000_0000)), (Some(ExtKind::RouteTarget), Some(ExtKind::RouteOrigin), None));
        assert_eq!(ext_text(0x0005_0000_0000_0001), "0x0005000000000001");
        assert!(ext_transitive(rt) && !ext_transitive(0x4002_0000_0000_0000));
        // What the console types parses to the same values, and nonsense does not parse.
        assert_eq!(ext_parse(ExtKind::RouteTarget, "65001:100"), Some(rt));
        assert_eq!(ext_parse(ExtKind::RouteOrigin, "10.0.0.1:5"), Some(soo));
        assert_eq!(ext_parse(ExtKind::RouteTarget, "4200000001:7"), Some(big));
        for bad in ["", "65001", "65001:", ":5", "65001:99999999999", "10.0.0.1:70000", "4200000001:70000", "a:b", "300.0.0.1:1"] {
            assert_eq!(ext_parse(ExtKind::RouteTarget, bad), None, "{bad}");
        }
        // They ride in the UPDATE as attribute 16, optional and transitive (flags 0xC0), eight octets each.
        let attrs =
            PathAttrs { as_path: vec![65_001], next_hop: Ipv4Addr::new(10, 0, 0, 1), ext_communities: vec![rt, soo, big], ..PathAttrs::default() };
        let u = Message::Update(Update { withdrawn: Vec::new(), attrs: Some(attrs.clone()), nlri: vec![net("10.0.0.0/8")] });
        let bytes = u.encode();
        assert!(bytes.windows(3).any(|w| w == [0xC0, ATTR_EXT_COMMUNITIES, 24]), "{bytes:?}");
        let Ok(Message::Update(back)) = Message::decode(&bytes) else { panic!("did not decode") };
        assert_eq!(back.attrs, Some(attrs));
    }

    #[test]
    fn a_hostile_extended_community_attribute_is_bounded_and_never_read_past_its_end() {
        // Forty values keep the first MAX_EXT_COMMUNITIES.
        let many: Vec<u64> = (1..=40).map(|i| ext_as2(ExtKind::RouteTarget, 1, i)).collect();
        let u = Message::Update(Update {
            withdrawn: Vec::new(),
            attrs: Some(PathAttrs { ext_communities: many, ..PathAttrs::default() }),
            nlri: vec![net("10.0.0.0/8")],
        });
        let Ok(Message::Update(back)) = Message::decode(&u.encode()) else { panic!("did not decode") };
        assert_eq!(back.attrs.map(|a| a.ext_communities.len()), Some(MAX_EXT_COMMUNITIES));
        // A value that is not a whole number of octets-by-eight drops the odd bytes; a short one reads as none.
        let odd = attribute(FLAG_OPTIONAL | FLAG_TRANSITIVE, ATTR_EXT_COMMUNITIES, &[0, 2, 0, 1, 0, 0, 0, 5, 9, 9, 9]);
        assert_eq!(read_attrs(&odd).map(|a| a.ext_communities), Ok(vec![ext_as2(ExtKind::RouteTarget, 1, 5)]));
        let short = attribute(FLAG_OPTIONAL | FLAG_TRANSITIVE, ATTR_EXT_COMMUNITIES, &[0, 2, 0]);
        assert_eq!(read_attrs(&short).map(|a| a.ext_communities), Ok(Vec::new()));
        // An attribute whose length runs past the data is an error, at every cut.
        let whole = attribute(FLAG_OPTIONAL | FLAG_TRANSITIVE, ATTR_EXT_COMMUNITIES, &ext_as2(ExtKind::RouteTarget, 1, 5).to_be_bytes());
        for n in 1..whole.len() {
            assert!(read_attrs(whole.get(..n).unwrap_or_default()).is_err(), "{n} bytes");
        }
    }

    #[test]
    fn route_reflection_and_confederation_attributes_have_their_own_codes() {
        let attrs = PathAttrs {
            origin: ORIGIN_IGP,
            as_path: vec![65_001],
            next_hop: Ipv4Addr::new(10, 0, 0, 1),
            originator_id: Some(Ipv4Addr::new(1, 2, 3, 4)),
            cluster_list: vec![Ipv4Addr::new(10, 10, 10, 10), Ipv4Addr::new(20, 20, 20, 20)],
            confed_path: vec![65_010, 65_011],
            confed_set: vec![65_012],
            ..PathAttrs::default()
        };
        let u = Message::Update(Update { withdrawn: Vec::new(), attrs: Some(attrs.clone()), nlri: vec![net("10.0.0.0/8")] });
        let bytes = u.encode();
        let Ok(Message::Update(back)) = Message::decode(&bytes) else { panic!("did not decode") };
        assert_eq!(back.attrs, Some(attrs));
        // ORIGINATOR_ID (9) and CLUSTER_LIST (10) are optional and not transitive (flags 0x80).
        assert!(bytes.windows(7).any(|w| w == [0x80, ATTR_ORIGINATOR_ID, 4, 1, 2, 3, 4]), "{bytes:?}");
        assert!(bytes.windows(3).any(|w| w == [0x80, ATTR_CLUSTER_LIST, 8]), "{bytes:?}");
        // The confederation segments (types 3 and 4) come before the AS_SEQUENCE (type 2).
        let confed = bytes.windows(2).position(|w| w == [SEG_CONFED_SEQUENCE, 2]);
        let seq = bytes.windows(2).position(|w| w == [SEG_AS_SEQUENCE, 1]);
        assert!(confed.is_some() && seq.is_some() && confed < seq, "{bytes:?}");
        assert!(bytes.windows(2).any(|w| w == [SEG_CONFED_SET, 1]));
    }

    #[test]
    fn a_hostile_cluster_list_and_originator_are_bounded_and_checked() {
        // A CLUSTER_LIST of 40 identifiers keeps the first MAX_CLUSTERS.
        let long: Vec<Ipv4Addr> = (1..=40).map(|i| Ipv4Addr::new(10, 0, 0, i)).collect();
        let u = Message::Update(Update {
            withdrawn: Vec::new(),
            attrs: Some(PathAttrs { cluster_list: long, ..PathAttrs::default() }),
            nlri: vec![net("10.0.0.0/8")],
        });
        let Ok(Message::Update(back)) = Message::decode(&u.encode()) else { panic!("did not decode") };
        assert_eq!(back.attrs.map(|a| a.cluster_list.len()), Some(MAX_CLUSTERS));
        // An ORIGINATOR_ID with fewer than four octets is truncated, not read past its end.
        let mut attrs = attribute(FLAG_OPTIONAL, ATTR_ORIGINATOR_ID, &[1, 2]);
        assert!(read_attrs(&attrs).is_err());
        // A CLUSTER_LIST whose length is not a multiple of four ignores the odd bytes.
        attrs = attribute(FLAG_OPTIONAL, ATTR_CLUSTER_LIST, &[1, 2, 3, 4, 5]);
        assert_eq!(read_attrs(&attrs).map(|a| a.cluster_list), Ok(vec![Ipv4Addr::new(1, 2, 3, 4)]));
    }

    #[test]
    fn describe_walks_several_messages_in_one_segment() {
        let mut data = Message::Keepalive.encode();
        data.extend(Message::Open(Open { asn: 65_001, hold_secs: 180, bgp_id: Ipv4Addr::new(1, 1, 1, 1) }).encode());
        assert_eq!(describe(&data), "KEEPALIVE, OPEN (AS 65001, hold time 180 s, id 1.1.1.1)");
    }
}
