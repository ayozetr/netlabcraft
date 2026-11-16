//! PPP over Ethernet (RFC 2516) and the PPP packets it carries: LCP and IPCP (RFC 1661, RFC
//! 1332), PAP (RFC 1334) and CHAP (RFC 1994).
//!
//! Two ethertypes: `0x8863` for the discovery stage (PADI, PADO, PADR, PADS and PADT, each with
//! a list of tags) and `0x8864` for the session stage, whose payload is a PPP protocol number
//! and a PPP packet. Every PPPoE packet starts with the version and type nibbles (both 1), a
//! code, a session id and the length of what follows. The length is what ends the packet:
//! Ethernet pads short frames, so bytes after it are ignored.
//!
//! Decoders treat their input as hostile: lengths are checked, the number of tags and options
//! is bounded, and nothing indexes a slice that was not measured first.

use std::net::Ipv4Addr;

use crate::md5::md5;
use crate::{Result, WireError, be16, ip_at};

/// The discovery stage.
pub const ETHERTYPE_DISCOVERY: u16 = 0x8863;
/// The session stage.
pub const ETHERTYPE_SESSION: u16 = 0x8864;

/// The largest PPP payload over PPPoE (RFC 2516 section 7: 1500 less the six bytes of the PPPoE
/// header and the two of the PPP protocol field), also the MRU and the IP MTU of a session.
pub const MRU: u16 = 1_492;

pub const CODE_PADI: u8 = 0x09;
pub const CODE_PADO: u8 = 0x07;
pub const CODE_PADR: u8 = 0x19;
pub const CODE_PADS: u8 = 0x65;
pub const CODE_PADT: u8 = 0xa7;
/// The code of every packet of the session stage.
pub const CODE_SESSION: u8 = 0x00;

pub const TAG_END_OF_LIST: u16 = 0x0000;
pub const TAG_SERVICE_NAME: u16 = 0x0101;
pub const TAG_AC_NAME: u16 = 0x0102;
pub const TAG_HOST_UNIQ: u16 = 0x0103;
pub const TAG_AC_COOKIE: u16 = 0x0104;
pub const TAG_SERVICE_NAME_ERROR: u16 = 0x0201;
pub const TAG_AC_SYSTEM_ERROR: u16 = 0x0202;
pub const TAG_GENERIC_ERROR: u16 = 0x0203;

/// Most tags a discovery packet is read with, and options a PPP packet is read with.
pub const MAX_TAGS: usize = 16;
pub const MAX_OPTIONS: usize = 16;

/// PPP protocol numbers.
pub const PPP_IP: u16 = 0x0021;
pub const PPP_IPCP: u16 = 0x8021;
pub const PPP_LCP: u16 = 0xc021;
pub const PPP_PAP: u16 = 0xc023;
pub const PPP_CHAP: u16 = 0xc223;

/// Codes of LCP and IPCP.
pub const CONF_REQ: u8 = 1;
pub const CONF_ACK: u8 = 2;
pub const CONF_NAK: u8 = 3;
pub const CONF_REJ: u8 = 4;
pub const TERM_REQ: u8 = 5;
pub const TERM_ACK: u8 = 6;
pub const ECHO_REQ: u8 = 9;
pub const ECHO_REP: u8 = 10;
/// Codes of PAP.
pub const PAP_REQUEST: u8 = 1;
pub const PAP_ACK: u8 = 2;
pub const PAP_NAK: u8 = 3;
/// Codes of CHAP.
pub const CHAP_CHALLENGE: u8 = 1;
pub const CHAP_RESPONSE: u8 = 2;
pub const CHAP_SUCCESS: u8 = 3;
pub const CHAP_FAILURE: u8 = 4;

/// Option types: LCP's maximum receive unit, authentication protocol and magic number; IPCP's
/// IP address.
pub const LCP_MRU: u8 = 1;
pub const LCP_AUTH: u8 = 3;
pub const LCP_MAGIC: u8 = 5;
pub const IPCP_ADDRESS: u8 = 3;

/// The only hash CHAP here uses (RFC 1994: MD5, algorithm 5).
const CHAP_MD5: u8 = 5;

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The PPPoE header and what it carries: the code, the session id and the payload.
fn parse_header<'a>(b: &'a [u8], what: &'static str) -> Result<(u8, u16, &'a [u8])> {
    let t = || WireError::Truncated(what);
    if *b.first().ok_or_else(t)? != 0x11 {
        return Err(WireError::Unsupported(what, "version or type is not 1".into()));
    }
    let code = *b.get(1).ok_or_else(t)?;
    let session = be16(b, 2).ok_or_else(t)?;
    let len = usize::from(be16(b, 4).ok_or_else(t)?);
    let end = 6usize.checked_add(len).ok_or_else(t)?;
    let payload = b.get(6..end).ok_or_else(t)?;
    Ok((code, session, payload))
}

fn with_header(code: u8, session: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x11, code];
    out.extend_from_slice(&session.to_be_bytes());
    out.extend_from_slice(&u16::try_from(payload.len()).unwrap_or(u16::MAX).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// One tag of a discovery packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub kind: u16,
    pub value: Vec<u8>,
}

impl Tag {
    pub fn new(kind: u16, value: &[u8]) -> Tag {
        Tag { kind, value: value.to_vec() }
    }

    pub fn text(kind: u16, value: &str) -> Tag {
        Tag { kind, value: value.as_bytes().to_vec() }
    }
}

/// A packet of the discovery stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub code: u8,
    pub session_id: u16,
    pub tags: Vec<Tag>,
}

impl Discovery {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        for tag in self.tags.iter().take(MAX_TAGS) {
            let value = tag.value.get(..tag.value.len().min(usize::from(u16::MAX))).unwrap_or_default();
            body.extend_from_slice(&tag.kind.to_be_bytes());
            body.extend_from_slice(&u16::try_from(value.len()).unwrap_or(u16::MAX).to_be_bytes());
            body.extend_from_slice(value);
        }
        with_header(self.code, self.session_id, &body)
    }

    pub fn decode(b: &[u8]) -> Result<Discovery> {
        let (code, session_id, body) = parse_header(b, "PPPoE discovery")?;
        if !matches!(code, CODE_PADI | CODE_PADO | CODE_PADR | CODE_PADS | CODE_PADT) {
            return Err(WireError::Unsupported("PPPoE discovery", format!("code 0x{code:02x}")));
        }
        let t = || WireError::Truncated("PPPoE discovery");
        let mut tags = Vec::new();
        let mut at = 0usize;
        while at < body.len() {
            let kind = be16(body, at).ok_or_else(t)?;
            let len = usize::from(be16(body, at + 2).ok_or_else(t)?);
            if kind == TAG_END_OF_LIST {
                break;
            }
            let start = at.checked_add(4).ok_or_else(t)?;
            let end = start.checked_add(len).ok_or_else(t)?;
            let value = body.get(start..end).ok_or_else(t)?.to_vec();
            if tags.len() >= MAX_TAGS {
                return Err(WireError::Unsupported("PPPoE discovery", "too many tags".into()));
            }
            tags.push(Tag { kind, value });
            at = end;
        }
        Ok(Discovery { code, session_id, tags })
    }

    /// The value of the first tag of this kind.
    pub fn tag(&self, kind: u16) -> Option<&[u8]> {
        self.tags.iter().find(|t| t.kind == kind).map(|t| t.value.as_slice())
    }

    pub fn tag_text(&self, kind: u16) -> Option<String> {
        self.tag(kind).map(text)
    }

    /// The text of the first error tag, when the packet carries one.
    pub fn error_text(&self) -> Option<String> {
        [TAG_SERVICE_NAME_ERROR, TAG_AC_SYSTEM_ERROR, TAG_GENERIC_ERROR]
            .iter()
            .find_map(|k| self.tag_text(*k))
            .map(|t| if t.is_empty() { "no reason given".to_string() } else { t })
    }

    /// The event-list wording.
    pub fn describe(&self) -> String {
        match self.code {
            CODE_PADI => "PPPoE PADI: looking for an access concentrator".to_string(),
            CODE_PADO => match self.tag_text(TAG_AC_NAME) {
                Some(n) if !n.is_empty() => format!("PPPoE PADO: access concentrator {n} offers its service"),
                _ => "PPPoE PADO: an access concentrator offers its service".to_string(),
            },
            CODE_PADR => "PPPoE PADR: asking for a session".to_string(),
            CODE_PADS if self.session_id == 0 => {
                format!("PPPoE PADS: refused ({})", self.error_text().unwrap_or_else(|| "no reason given".to_string()))
            }
            CODE_PADS => format!("PPPoE PADS: session 0x{:04x} granted", self.session_id),
            CODE_PADT => format!("PPPoE PADT: session 0x{:04x} ended", self.session_id),
            other => format!("PPPoE discovery code 0x{other:02x}"),
        }
    }
}

/// A packet of the session stage: a PPP protocol number and its data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPacket {
    pub session_id: u16,
    pub protocol: u16,
    pub data: Vec<u8>,
}

impl SessionPacket {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = self.protocol.to_be_bytes().to_vec();
        body.extend_from_slice(&self.data);
        with_header(CODE_SESSION, self.session_id, &body)
    }

    pub fn decode(b: &[u8]) -> Result<SessionPacket> {
        let (code, session_id, body) = parse_header(b, "PPPoE session")?;
        if code != CODE_SESSION {
            return Err(WireError::Unsupported("PPPoE session", format!("code 0x{code:02x}")));
        }
        let protocol = be16(body, 0).ok_or(WireError::Truncated("PPPoE session"))?;
        let data = body.get(2..).unwrap_or_default().to_vec();
        Ok(SessionPacket { session_id, protocol, data })
    }

    /// The event-list wording (an IPv4 datagram is described by the caller, which can read it).
    pub fn describe(&self) -> String {
        let what = match self.protocol {
            PPP_IP => format!("IPv4 datagram, {} bytes", self.data.len()),
            PPP_LCP | PPP_IPCP | PPP_PAP | PPP_CHAP => match Control::decode(&self.data) {
                Ok(c) => c.describe(self.protocol),
                Err(e) => format!("{} ({e})", protocol_name(self.protocol)),
            },
            other => format!("PPP protocol 0x{other:04x}"),
        };
        format!("PPPoE session 0x{:04x}: {what}", self.session_id)
    }
}

fn protocol_name(protocol: u16) -> &'static str {
    match protocol {
        PPP_LCP => "LCP",
        PPP_IPCP => "IPCP",
        PPP_PAP => "PAP",
        PPP_CHAP => "CHAP",
        PPP_IP => "IPv4",
        _ => "PPP",
    }
}

fn code_name(protocol: u16, code: u8) -> String {
    let name = match (protocol, code) {
        (PPP_LCP | PPP_IPCP, CONF_REQ) => "Configure-Request",
        (PPP_LCP | PPP_IPCP, CONF_ACK) => "Configure-Ack",
        (PPP_LCP | PPP_IPCP, CONF_NAK) => "Configure-Nak",
        (PPP_LCP | PPP_IPCP, CONF_REJ) => "Configure-Reject",
        (PPP_LCP | PPP_IPCP, TERM_REQ) => "Terminate-Request",
        (PPP_LCP | PPP_IPCP, TERM_ACK) => "Terminate-Ack",
        (PPP_LCP, ECHO_REQ) => "Echo-Request",
        (PPP_LCP, ECHO_REP) => "Echo-Reply",
        (PPP_PAP, PAP_REQUEST) => "Authenticate-Request",
        (PPP_PAP, PAP_ACK) => "Authenticate-Ack",
        (PPP_PAP, PAP_NAK) => "Authenticate-Nak",
        (PPP_CHAP, CHAP_CHALLENGE) => "Challenge",
        (PPP_CHAP, CHAP_RESPONSE) => "Response",
        (PPP_CHAP, CHAP_SUCCESS) => "Success",
        (PPP_CHAP, CHAP_FAILURE) => "Failure",
        _ => return format!("code {code}"),
    };
    name.to_string()
}

/// A PPP control packet (the shape LCP, IPCP, PAP and CHAP share): a code, an identifier and
/// the data, whose length field counts the four header bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub code: u8,
    pub id: u8,
    pub data: Vec<u8>,
}

impl Control {
    pub fn new(code: u8, id: u8, data: Vec<u8>) -> Control {
        Control { code, id, data }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![self.code, self.id];
        out.extend_from_slice(&u16::try_from(self.data.len().saturating_add(4)).unwrap_or(u16::MAX).to_be_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Control> {
        let t = || WireError::Truncated("PPP");
        let code = *b.first().ok_or_else(t)?;
        let id = *b.get(1).ok_or_else(t)?;
        let len = usize::from(be16(b, 2).ok_or_else(t)?);
        if len < 4 {
            return Err(WireError::Unsupported("PPP", format!("length {len}")));
        }
        let data = b.get(4..len).ok_or_else(t)?.to_vec();
        Ok(Control { code, id, data })
    }

    /// The event-list wording of this packet of `protocol`.
    pub fn describe(&self, protocol: u16) -> String {
        let name = protocol_name(protocol);
        let what = code_name(protocol, self.code);
        let extra = match (protocol, self.code) {
            (PPP_LCP | PPP_IPCP, CONF_REQ | CONF_ACK | CONF_NAK | CONF_REJ) => match decode_options(&self.data) {
                Ok(opts) if !opts.is_empty() => format!(" ({})", opts.iter().map(|o| option_text(protocol, o)).collect::<Vec<_>>().join(", ")),
                _ => String::new(),
            },
            (PPP_PAP, PAP_REQUEST) => parse_pap_request(&self.data).map(|(u, _)| format!(" (user {u})")).unwrap_or_default(),
            (PPP_CHAP, CHAP_CHALLENGE | CHAP_RESPONSE) => {
                parse_chap_data(&self.data).map(|(_, n)| if n.is_empty() { String::new() } else { format!(" (name {n})") }).unwrap_or_default()
            }
            _ => String::new(),
        };
        format!("{name} {what}{extra}")
    }
}

/// One option of an LCP or IPCP packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opt {
    pub kind: u8,
    pub data: Vec<u8>,
}

pub fn encode_options(opts: &[Opt]) -> Vec<u8> {
    let mut out = Vec::new();
    for o in opts.iter().take(MAX_OPTIONS) {
        let data = o.data.get(..o.data.len().min(253)).unwrap_or_default();
        out.push(o.kind);
        out.push(u8::try_from(data.len().saturating_add(2)).unwrap_or(u8::MAX));
        out.extend_from_slice(data);
    }
    out
}

pub fn decode_options(b: &[u8]) -> Result<Vec<Opt>> {
    let t = || WireError::Truncated("PPP option");
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < b.len() {
        let kind = *b.get(at).ok_or_else(t)?;
        let len = usize::from(*b.get(at + 1).ok_or_else(t)?);
        if len < 2 {
            return Err(WireError::Unsupported("PPP option", format!("length {len}")));
        }
        let end = at.checked_add(len).ok_or_else(t)?;
        let data = b.get(at + 2..end).ok_or_else(t)?.to_vec();
        if out.len() >= MAX_OPTIONS {
            return Err(WireError::Unsupported("PPP option", "too many options".into()));
        }
        out.push(Opt { kind, data });
        at = end;
    }
    Ok(out)
}

/// The data of the first option of this type.
pub fn opt(opts: &[Opt], kind: u8) -> Option<&[u8]> {
    opts.iter().find(|o| o.kind == kind).map(|o| o.data.as_slice())
}

pub fn mru_option(mru: u16) -> Opt {
    Opt { kind: LCP_MRU, data: mru.to_be_bytes().to_vec() }
}

pub fn magic_option(magic: u32) -> Opt {
    Opt { kind: LCP_MAGIC, data: magic.to_be_bytes().to_vec() }
}

pub fn address_option(addr: Ipv4Addr) -> Opt {
    Opt { kind: IPCP_ADDRESS, data: addr.octets().to_vec() }
}

/// The authentication-protocol option: CHAP with MD5 (`true`) or PAP.
pub fn auth_option(chap: bool) -> Opt {
    let mut data = if chap { PPP_CHAP } else { PPP_PAP }.to_be_bytes().to_vec();
    if chap {
        data.push(CHAP_MD5);
    }
    Opt { kind: LCP_AUTH, data }
}

/// Reads an authentication-protocol option: `Some(true)` for CHAP with MD5, `Some(false)` for
/// PAP, `None` for anything else.
pub fn parse_auth(data: &[u8]) -> Option<bool> {
    match (be16(data, 0)?, data.get(2)) {
        (PPP_PAP, _) => Some(false),
        (PPP_CHAP, Some(&CHAP_MD5)) => Some(true),
        _ => None,
    }
}

pub fn opt_mru(opts: &[Opt]) -> Option<u16> {
    be16(opt(opts, LCP_MRU)?, 0)
}

pub fn opt_address(opts: &[Opt]) -> Option<Ipv4Addr> {
    ip_at(opt(opts, IPCP_ADDRESS)?, 0)
}

fn option_text(protocol: u16, o: &Opt) -> String {
    match (protocol, o.kind) {
        (PPP_LCP, LCP_MRU) => be16(&o.data, 0).map_or_else(|| "MRU".to_string(), |v| format!("MRU {v}")),
        (PPP_LCP, LCP_AUTH) => match parse_auth(&o.data) {
            Some(true) => "CHAP".to_string(),
            Some(false) => "PAP".to_string(),
            None => "authentication".to_string(),
        },
        (PPP_LCP, LCP_MAGIC) => "magic number".to_string(),
        (PPP_IPCP, IPCP_ADDRESS) => ip_at(&o.data, 0).map_or_else(|| "address".to_string(), |a| format!("address {a}")),
        _ => format!("option {}", o.kind),
    }
}

/// The data of a PAP Authenticate-Request: the peer id and the password, each after its length.
pub fn pap_request(user: &str, password: &str) -> Vec<u8> {
    let user: Vec<u8> = user.bytes().take(255).collect();
    let password: Vec<u8> = password.bytes().take(255).collect();
    let mut out = vec![u8::try_from(user.len()).unwrap_or(0)];
    out.extend_from_slice(&user);
    out.push(u8::try_from(password.len()).unwrap_or(0));
    out.extend_from_slice(&password);
    out
}

pub fn parse_pap_request(d: &[u8]) -> Result<(String, String)> {
    let t = || WireError::Truncated("PAP");
    let ulen = usize::from(*d.first().ok_or_else(t)?);
    let user = d.get(1..1 + ulen).ok_or_else(t)?;
    let plen = usize::from(*d.get(1 + ulen).ok_or_else(t)?);
    let password = d.get(2 + ulen..2 + ulen + plen).ok_or_else(t)?;
    Ok((text(user), text(password)))
}

/// The data of a PAP Ack or Nak: a message after its length.
pub fn message(msg: &str) -> Vec<u8> {
    let bytes: Vec<u8> = msg.bytes().take(255).collect();
    let mut out = vec![u8::try_from(bytes.len()).unwrap_or(0)];
    out.extend_from_slice(&bytes);
    out
}

/// The data of a CHAP Challenge or Response: the value after its length, then the name.
pub fn chap_data(value: &[u8], name: &str) -> Vec<u8> {
    let value: Vec<u8> = value.iter().copied().take(255).collect();
    let mut out = vec![u8::try_from(value.len()).unwrap_or(0)];
    out.extend_from_slice(&value);
    out.extend_from_slice(name.as_bytes());
    out
}

pub fn parse_chap_data(d: &[u8]) -> Result<(Vec<u8>, String)> {
    let t = || WireError::Truncated("CHAP");
    let len = usize::from(*d.first().ok_or_else(t)?);
    let value = d.get(1..1 + len).ok_or_else(t)?.to_vec();
    let name = d.get(1 + len..).unwrap_or_default();
    Ok((value, text(name)))
}

/// The CHAP response (RFC 1994 section 4.1): MD5 of the identifier, the secret and the challenge.
pub fn chap_response(id: u8, secret: &str, challenge: &[u8]) -> [u8; 16] {
    let mut m = vec![id];
    m.extend_from_slice(secret.as_bytes());
    m.extend_from_slice(challenge);
    md5(&m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discoveries() -> Vec<Discovery> {
        vec![
            Discovery { code: CODE_PADI, session_id: 0, tags: vec![Tag::new(TAG_SERVICE_NAME, b""), Tag::new(TAG_HOST_UNIQ, &[1, 2, 3, 4])] },
            Discovery {
                code: CODE_PADO,
                session_id: 0,
                tags: vec![Tag::text(TAG_AC_NAME, "ISP1"), Tag::new(TAG_HOST_UNIQ, &[1, 2, 3, 4]), Tag::new(TAG_AC_COOKIE, &[9; 8])],
            },
            Discovery { code: CODE_PADR, session_id: 0, tags: vec![Tag::new(TAG_AC_COOKIE, &[9; 8])] },
            Discovery { code: CODE_PADS, session_id: 7, tags: vec![Tag::new(TAG_SERVICE_NAME, b"")] },
            Discovery { code: CODE_PADS, session_id: 0, tags: vec![Tag::text(TAG_AC_SYSTEM_ERROR, "no free address")] },
            Discovery { code: CODE_PADT, session_id: 7, tags: vec![] },
        ]
    }

    #[test]
    fn every_discovery_packet_round_trips() {
        for m in discoveries() {
            assert_eq!(Discovery::decode(&m.encode()).unwrap(), m);
        }
    }

    #[test]
    fn the_header_is_the_one_of_the_rfc() {
        let b = Discovery { code: CODE_PADS, session_id: 0x1234, tags: vec![Tag::new(TAG_SERVICE_NAME, b"ab")] }.encode();
        assert_eq!(b, vec![0x11, 0x65, 0x12, 0x34, 0x00, 0x06, 0x01, 0x01, 0x00, 0x02, b'a', b'b']);
        // Ethernet padding after the length is ignored.
        let mut padded = b.clone();
        padded.extend_from_slice(&[0; 20]);
        assert_eq!(Discovery::decode(&padded).unwrap().tag(TAG_SERVICE_NAME), Some(&b"ab"[..]));
    }

    #[test]
    fn the_end_of_list_tag_stops_the_list() {
        let mut b = Discovery { code: CODE_PADI, session_id: 0, tags: vec![Tag::new(TAG_HOST_UNIQ, &[5])] }.encode();
        b.extend_from_slice(&[0, 0, 0, 0, 0x01, 0x01, 0x00, 0x00]);
        let len = u16::try_from(b.len() - 6).unwrap().to_be_bytes();
        b[4] = len[0];
        b[5] = len[1];
        let d = Discovery::decode(&b).unwrap();
        assert_eq!(d.tags.len(), 1);
    }

    #[test]
    fn session_packets_round_trip() {
        let c = Control::new(CONF_REQ, 3, encode_options(&[mru_option(MRU), auth_option(true), magic_option(0x0102_0304)]));
        let s = SessionPacket { session_id: 9, protocol: PPP_LCP, data: c.encode() };
        let back = SessionPacket::decode(&s.encode()).unwrap();
        assert_eq!(back, s);
        assert_eq!(Control::decode(&back.data).unwrap(), c);
        let opts = decode_options(&c.data).unwrap();
        assert_eq!(opt_mru(&opts), Some(MRU));
        assert_eq!(parse_auth(opt(&opts, LCP_AUTH).unwrap()), Some(true));
        assert_eq!(parse_auth(&auth_option(false).data), Some(false));
        assert!(back.describe().contains("LCP Configure-Request (MRU 1492, CHAP, magic number)"), "{}", back.describe());
    }

    #[test]
    fn the_event_list_reads_both_stages() {
        let mac = |n: u8| netlabcraft_core::MacAddr([2, 0, 0, 0, 0, n]);
        let padi = Discovery { code: CODE_PADI, session_id: 0, tags: vec![] };
        let f = crate::Frame {
            dst: netlabcraft_core::MacAddr::BROADCAST,
            src: mac(1),
            vlan: None,
            ethertype: ETHERTYPE_DISCOVERY,
            payload: padi.encode(),
        };
        let (p, s) = crate::summarize(&f.encode());
        assert_eq!((p, s.as_str()), (crate::Protocol::Pppoe, "PPPoE PADI: looking for an access concentrator"));
        let lcp = Control::new(ECHO_REQ, 1, vec![0; 4]);
        let f = crate::Frame {
            dst: mac(2),
            src: mac(1),
            vlan: None,
            ethertype: ETHERTYPE_SESSION,
            payload: SessionPacket { session_id: 3, protocol: PPP_LCP, data: lcp.encode() }.encode(),
        };
        let (p, s) = crate::summarize(&f.encode());
        assert_eq!((p, s.as_str()), (crate::Protocol::Pppoe, "PPPoE session 0x0003: LCP Echo-Request"));
    }

    #[test]
    fn ipcp_carries_an_address() {
        let a = Ipv4Addr::new(10, 9, 0, 100);
        let opts = decode_options(&encode_options(&[address_option(a)])).unwrap();
        assert_eq!(opt_address(&opts), Some(a));
        let c = Control::new(CONF_NAK, 1, encode_options(&[address_option(a)]));
        assert_eq!(c.describe(PPP_IPCP), "IPCP Configure-Nak (address 10.9.0.100)");
    }

    #[test]
    fn pap_and_chap_data_round_trip() {
        let d = pap_request("ann", "pw-test");
        assert_eq!(parse_pap_request(&d).unwrap(), ("ann".to_string(), "pw-test".to_string()));
        assert_eq!(message("ok"), vec![2, b'o', b'k']);
        let c = chap_data(&[1, 2, 3], "ISP1");
        assert_eq!(parse_chap_data(&c).unwrap(), (vec![1, 2, 3], "ISP1".to_string()));
        let mut m = vec![7u8];
        m.extend_from_slice(b"pw-test");
        m.extend_from_slice(&[1, 2, 3]);
        assert_eq!(chap_response(7, "pw-test", &[1, 2, 3]), md5(&m));
        assert_ne!(chap_response(7, "pw-tesT", &[1, 2, 3]), md5(&m));
        let ctl = Control::new(PAP_REQUEST, 1, d);
        assert_eq!(ctl.describe(PPP_PAP), "PAP Authenticate-Request (user ann)");
    }

    #[test]
    fn every_truncation_and_hostile_byte_is_an_error_not_a_panic() {
        let mut all: Vec<Vec<u8>> = discoveries().iter().map(Discovery::encode).collect();
        let c = Control::new(CONF_REQ, 1, encode_options(&[mru_option(MRU), magic_option(1)]));
        all.push(SessionPacket { session_id: 1, protocol: PPP_LCP, data: c.encode() }.encode());
        all.push(Control::new(PAP_REQUEST, 1, pap_request("ann", "pw-test")).encode());
        all.push(Control::new(CHAP_RESPONSE, 1, chap_data(&[1; 16], "ann")).encode());
        for bytes in all {
            for n in 0..bytes.len() {
                let _ = Discovery::decode(&bytes[..n]);
                let _ = SessionPacket::decode(&bytes[..n]);
                let _ = Control::decode(&bytes[..n]);
                let _ = decode_options(&bytes[..n]);
                let _ = parse_pap_request(&bytes[..n]);
                let _ = parse_chap_data(&bytes[..n]);
            }
            for i in 0..bytes.len() {
                let mut x = bytes.clone();
                x[i] = x[i].wrapping_add(0x55);
                let _ = Discovery::decode(&x);
                let _ = SessionPacket::decode(&x);
                let _ = Control::decode(&x);
                let _ = decode_options(&x);
            }
        }
        assert!(Discovery::decode(&[]).is_err());
        assert!(Discovery::decode(&[0x11, CODE_PADI, 0, 0, 0, 9, 1, 1]).is_err());
        assert!(Discovery::decode(&[0x21, CODE_PADI, 0, 0, 0, 0]).is_err());
        assert!(Discovery::decode(&[0x11, 0x42, 0, 0, 0, 0]).is_err());
        assert!(SessionPacket::decode(&[0x11, CODE_SESSION, 0, 1, 0, 1, 0xc0]).is_err());
        assert!(Control::decode(&[1, 1, 0, 3]).is_err());
        assert!(decode_options(&[3, 1]).is_err());
        // A tag list longer than the bound is refused, not read.
        let mut body = Vec::new();
        for _ in 0..=MAX_TAGS {
            body.extend_from_slice(&[0x01, 0x03, 0x00, 0x00]);
        }
        let mut b = vec![0x11, CODE_PADI, 0, 0];
        b.extend_from_slice(&u16::try_from(body.len()).unwrap().to_be_bytes());
        b.extend_from_slice(&body);
        assert!(Discovery::decode(&b).is_err());
    }
}
