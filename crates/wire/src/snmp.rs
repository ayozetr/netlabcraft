//! SNMPv2c (RFC 3416, BER-encoded as in RFC 1157): GetRequest, GetNextRequest, SetRequest,
//! the Response and the SNMPv2-Trap, with the community string; and SNMPv3 messages (RFC 3412)
//! with the user-based security model (RFC 3414): no authentication, or HMAC-MD5-96 and
//! HMAC-SHA-96 with localized keys. No privacy (encryption).

use crate::sha1::{Hash, hmac};
use crate::{Result, WireError};

pub const SNMP_PORT: u16 = 161;
/// Where managers listen for notifications.
pub const SNMP_TRAP_PORT: u16 = 162;

/// Well-known object identifiers (RFC 3418, RFC 2863, RFC 3414).
pub mod oids {
    pub const SYS_UPTIME: &[u32] = &[1, 3, 6, 1, 2, 1, 1, 3, 0];
    pub const SNMP_TRAP_OID: &[u32] = &[1, 3, 6, 1, 6, 3, 1, 1, 4, 1, 0];
    pub const COLD_START: &[u32] = &[1, 3, 6, 1, 6, 3, 1, 1, 5, 1];
    pub const LINK_DOWN: &[u32] = &[1, 3, 6, 1, 6, 3, 1, 1, 5, 3];
    pub const LINK_UP: &[u32] = &[1, 3, 6, 1, 6, 3, 1, 1, 5, 4];
    /// ifIndex, ifAdminStatus, ifOperStatus columns (the row number follows).
    pub const IF_INDEX: &[u32] = &[1, 3, 6, 1, 2, 1, 2, 2, 1, 1];
    pub const IF_ADMIN: &[u32] = &[1, 3, 6, 1, 2, 1, 2, 2, 1, 7];
    pub const IF_OPER: &[u32] = &[1, 3, 6, 1, 2, 1, 2, 2, 1, 8];
    /// usmStats counters a Report carries.
    pub const UNSUPPORTED_SEC_LEVELS: &[u32] = &[1, 3, 6, 1, 6, 3, 15, 1, 1, 1, 0];
    pub const UNKNOWN_USER_NAMES: &[u32] = &[1, 3, 6, 1, 6, 3, 15, 1, 1, 3, 0];
    pub const UNKNOWN_ENGINE_IDS: &[u32] = &[1, 3, 6, 1, 6, 3, 15, 1, 1, 4, 0];
    pub const WRONG_DIGESTS: &[u32] = &[1, 3, 6, 1, 6, 3, 15, 1, 1, 5, 0];

    /// A notification's name, for people.
    pub fn trap_name(oid: &[u32]) -> Option<&'static str> {
        match oid {
            COLD_START => Some("coldStart"),
            LINK_DOWN => Some("linkDown"),
            LINK_UP => Some("linkUp"),
            _ => None,
        }
    }
}

/// A value in a variable binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Integer(i64),
    Text(String),
    Oid(Vec<u32>),
    /// Hundredths of a second.
    TimeTicks(u32),
    Counter(u32),
    /// End of the MIB (GetNext past the last object).
    EndOfMib,
    NoSuchObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PduKind {
    Get,
    GetNext,
    Response,
    Set,
    /// SNMPv2-Trap: a notification an agent sends unasked.
    Trap,
    /// SNMPv3: an engine reports a problem with a message (or its engine ID, for discovery).
    Report,
}

impl PduKind {
    fn tag(self) -> u8 {
        match self {
            PduKind::Get => 0xa0,
            PduKind::GetNext => 0xa1,
            PduKind::Response => 0xa2,
            PduKind::Set => 0xa3,
            PduKind::Trap => 0xa7,
            PduKind::Report => 0xa8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PduKind::Get => "get-request",
            PduKind::GetNext => "get-next-request",
            PduKind::Response => "response",
            PduKind::Set => "set-request",
            PduKind::Trap => "trap",
            PduKind::Report => "report",
        }
    }
}

/// Error statuses a Response carries.
pub const NO_ERROR: u8 = 0;
pub const NO_ACCESS: u8 = 6;
pub const NOT_WRITABLE: u8 = 17;
pub const WRONG_TYPE: u8 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snmp {
    pub community: String,
    pub kind: PduKind,
    pub request_id: i32,
    pub error_status: u8,
    pub error_index: u8,
    pub bindings: Vec<(Vec<u32>, Value)>,
}

fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = body.len();
    if n < 0x80 {
        out.push(n as u8);
    } else if n < 0x100 {
        out.extend_from_slice(&[0x81, n as u8]);
    } else {
        out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]);
    }
    out.extend_from_slice(body);
    out
}

fn int(v: i64) -> Vec<u8> {
    let b = v.to_be_bytes();
    // The shortest two's-complement form.
    let mut i = 0;
    while i < 7 && ((b[i] == 0 && b[i + 1] & 0x80 == 0) || (b[i] == 0xff && b[i + 1] & 0x80 != 0)) {
        i += 1;
    }
    b.get(i..).unwrap_or_default().to_vec()
}

fn oid_bytes(oid: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    match oid {
        [a, b, rest @ ..] => {
            out.push((a * 40 + b) as u8);
            for &n in rest {
                let mut groups = vec![(n & 0x7f) as u8];
                let mut v = n >> 7;
                while v > 0 {
                    groups.push(((v & 0x7f) as u8) | 0x80);
                    v >>= 7;
                }
                groups.reverse();
                out.extend(groups);
            }
        }
        [a] => out.push((a * 40) as u8),
        [] => {}
    }
    out
}

impl Value {
    fn encode(&self) -> Vec<u8> {
        match self {
            Value::Null => tlv(0x05, &[]),
            Value::Integer(v) => tlv(0x02, &int(*v)),
            Value::Text(s) => tlv(0x04, s.as_bytes()),
            Value::Oid(o) => tlv(0x06, &oid_bytes(o)),
            Value::TimeTicks(t) => tlv(0x43, &int(i64::from(*t))),
            Value::Counter(c) => tlv(0x41, &int(i64::from(*c))),
            Value::EndOfMib => tlv(0x82, &[]),
            Value::NoSuchObject => tlv(0x80, &[]),
        }
    }

    /// The value as a person reads it.
    pub fn show(&self) -> String {
        match self {
            Value::Null => "(null)".into(),
            Value::Integer(v) => v.to_string(),
            Value::Text(s) => s.clone(),
            Value::Oid(o) => oid_text(o),
            Value::TimeTicks(t) => format!("{t} ({} s)", t / 100),
            Value::Counter(c) => c.to_string(),
            Value::EndOfMib => "end of the MIB".into(),
            Value::NoSuchObject => "no such object".into(),
        }
    }
}

/// `1.3.6.1.2.1.1.5.0` as numbers.
pub fn parse_oid(s: &str) -> Option<Vec<u32>> {
    let v: Option<Vec<u32>> = s.trim().trim_start_matches('.').split('.').map(|x| x.parse().ok()).collect();
    v.filter(|o| o.len() >= 2 && o.len() <= 64)
}

pub fn oid_text(o: &[u32]) -> String {
    o.iter().map(u32::to_string).collect::<Vec<_>>().join(".")
}

impl Snmp {
    /// The PDU alone (what SNMPv3 wraps in its scoped PDU).
    fn pdu(&self) -> Vec<u8> {
        let mut binds = Vec::new();
        for (oid, v) in &self.bindings {
            binds.extend(tlv(0x30, &[tlv(0x06, &oid_bytes(oid)), v.encode()].concat()));
        }
        let body = [
            tlv(0x02, &int(i64::from(self.request_id))),
            tlv(0x02, &int(i64::from(self.error_status))),
            tlv(0x02, &int(i64::from(self.error_index))),
            tlv(0x30, &binds),
        ]
        .concat();
        tlv(self.kind.tag(), &body)
    }

    pub fn encode(&self) -> Vec<u8> {
        // Version 1 means SNMPv2c.
        tlv(0x30, &[tlv(0x02, &[1]), tlv(0x04, self.community.as_bytes()), self.pdu()].concat())
    }

    pub fn decode(b: &[u8]) -> Result<Snmp> {
        let mut r = Reader { b, at: 0, base: 0 };
        let mut msg = r.enter(0x30)?;
        let _version = msg.integer()?;
        let community = String::from_utf8_lossy(msg.take(0x04)?).into_owned();
        let mut m = decode_pdu(&mut msg)?;
        m.community = community;
        Ok(m)
    }
}

/// The message's version field: 0 SNMPv1, 1 SNMPv2c, 3 SNMPv3.
pub fn version(b: &[u8]) -> Option<i64> {
    let mut r = Reader { b, at: 0, base: 0 };
    r.enter(0x30).ok()?.integer().ok()
}

fn decode_pdu(msg: &mut Reader<'_>) -> Result<Snmp> {
    let tag = *msg.b.get(msg.at).ok_or(WireError::Truncated("snmp"))?;
    let kind = match tag {
        0xa0 => PduKind::Get,
        0xa1 => PduKind::GetNext,
        0xa2 => PduKind::Response,
        0xa3 => PduKind::Set,
        0xa7 => PduKind::Trap,
        0xa8 => PduKind::Report,
        t => return Err(WireError::Unsupported("snmp", format!("PDU type 0x{t:02x}"))),
    };
    let mut pdu = msg.enter(tag)?;
    let request_id = i32::try_from(pdu.integer()?).unwrap_or(0);
    let error_status = u8::try_from(pdu.integer()?).unwrap_or(0);
    let error_index = u8::try_from(pdu.integer()?).unwrap_or(0);
    let mut list = pdu.enter(0x30)?;
    let mut bindings = Vec::new();
    while list.at < list.b.len() && bindings.len() < 64 {
        let mut vb = list.enter(0x30)?;
        let oid = decode_oid(vb.take(0x06)?);
        let (t, body, _) = vb.any()?;
        let value = match t {
            0x05 => Value::Null,
            0x02 => Value::Integer(be_int(body)),
            0x04 => Value::Text(String::from_utf8_lossy(body).into_owned()),
            0x06 => Value::Oid(decode_oid(body)),
            0x43 => Value::TimeTicks(u32::try_from(be_int(body) & 0xffff_ffff).unwrap_or(0)),
            0x41 => Value::Counter(u32::try_from(be_int(body) & 0xffff_ffff).unwrap_or(0)),
            0x82 => Value::EndOfMib,
            _ => Value::NoSuchObject,
        };
        bindings.push((oid, value));
    }
    Ok(Snmp { community: String::new(), kind, request_id, error_status, error_index, bindings })
}

/// SNMPv3 message flags (RFC 3412 §6.4).
pub const FLAG_AUTH: u8 = 1;
pub const FLAG_PRIV: u8 = 2;
pub const FLAG_REPORTABLE: u8 = 4;
/// Bytes of the truncated HMAC in an authenticated message.
pub const AUTH_LEN: usize = 12;

/// An SNMPv3 message with the user-based security model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V3 {
    pub msg_id: i32,
    pub flags: u8,
    /// The authoritative engine (the agent's, for requests to it).
    pub engine_id: Vec<u8>,
    pub boots: u32,
    pub time: u32,
    pub user: String,
    /// The HMAC (12 bytes) when authenticated, empty otherwise.
    pub auth: Vec<u8>,
    pub context_name: String,
    /// The PDU; its community is unused.
    pub pdu: Snmp,
}

impl V3 {
    pub fn encode(&self) -> Vec<u8> {
        let header = [
            tlv(0x02, &int(i64::from(self.msg_id))),
            tlv(0x02, &int(65_507)),
            tlv(0x04, &[self.flags]),
            // The user-based security model.
            tlv(0x02, &[3]),
        ]
        .concat();
        let usm = tlv(
            0x30,
            &[
                tlv(0x04, &self.engine_id),
                tlv(0x02, &int(i64::from(self.boots))),
                tlv(0x02, &int(i64::from(self.time))),
                tlv(0x04, self.user.as_bytes()),
                tlv(0x04, &self.auth),
                tlv(0x04, &[]),
            ]
            .concat(),
        );
        let scoped = tlv(0x30, &[tlv(0x04, &self.engine_id), tlv(0x04, self.context_name.as_bytes()), self.pdu.pdu()].concat());
        tlv(0x30, &[tlv(0x02, &[3]), tlv(0x30, &header), tlv(0x04, &usm), scoped].concat())
    }

    /// The message, and where its authentication parameters start in `b`.
    pub fn decode(b: &[u8]) -> Result<(V3, usize)> {
        let mut r = Reader { b, at: 0, base: 0 };
        let mut msg = r.enter(0x30)?;
        if msg.integer()? != 3 {
            return Err(WireError::Unsupported("snmp", "not SNMPv3".into()));
        }
        let mut header = msg.enter(0x30)?;
        let msg_id = i32::try_from(header.integer()?).unwrap_or(0);
        let _max = header.integer()?;
        let flags = header.take(0x04)?.first().copied().unwrap_or(0);
        if header.integer()? != 3 {
            return Err(WireError::Unsupported("snmp", "security model".into()));
        }
        let mut params = msg.enter(0x04)?;
        let mut usm = params.enter(0x30)?;
        let engine_id = usm.take(0x04)?.to_vec();
        let boots = u32::try_from(usm.integer()?).unwrap_or(0);
        let time = u32::try_from(usm.integer()?).unwrap_or(0);
        let user = String::from_utf8_lossy(usm.take(0x04)?).into_owned();
        let (auth, auth_at) = usm.take_at(0x04)?;
        let _privacy = usm.take(0x04)?;
        if flags & FLAG_PRIV != 0 {
            return Err(WireError::Unsupported("snmp", "encrypted (privacy) messages".into()));
        }
        let mut scoped = msg.enter(0x30)?;
        let _context_engine = scoped.take(0x04)?;
        let context_name = String::from_utf8_lossy(scoped.take(0x04)?).into_owned();
        let pdu = decode_pdu(&mut scoped)?;
        let m = V3 { msg_id, flags, engine_id, boots, time, user, auth: auth.to_vec(), context_name, pdu };
        Ok((m, auth_at))
    }

    /// Encoded and authenticated with `key` (a localized key, see [`localize_key`]).
    pub fn encode_signed(&self, hash: Hash, key: &[u8]) -> Vec<u8> {
        let blank = V3 { auth: vec![0; AUTH_LEN], flags: self.flags | FLAG_AUTH, ..self.clone() };
        let mut b = blank.encode();
        let Ok((_, at)) = V3::decode(&b) else { return b };
        let mac = hmac(hash, key, &b);
        if let (Some(dst), Some(src)) = (b.get_mut(at..at + AUTH_LEN), mac.get(..AUTH_LEN)) {
            dst.copy_from_slice(src);
        }
        b
    }

    /// Whether an authenticated message's HMAC is right for `key`.
    pub fn verify(b: &[u8], hash: Hash, key: &[u8]) -> bool {
        let Ok((m, at)) = V3::decode(b) else { return false };
        if m.auth.len() != AUTH_LEN {
            return false;
        }
        let mut blank = b.to_vec();
        if let Some(x) = blank.get_mut(at..at + AUTH_LEN) {
            x.fill(0);
        }
        hmac(hash, key, &blank).get(..AUTH_LEN) == Some(m.auth.as_slice())
    }
}

/// RFC 3414 A.2: the password stretched to a key (a megabyte of it hashed), then localized to
/// one engine. Empty for an empty password.
pub fn localize_key(hash: Hash, password: &str, engine_id: &[u8]) -> Vec<u8> {
    let p = password.as_bytes();
    if p.is_empty() {
        return Vec::new();
    }
    let stream: Vec<u8> = p.iter().copied().cycle().take(1_048_576).collect();
    let ku = hash.digest(&stream);
    hash.digest(&[ku.as_slice(), engine_id, ku.as_slice()].concat())
}

fn be_int(b: &[u8]) -> i64 {
    let mut v: i64 = if b.first().is_some_and(|x| x & 0x80 != 0) { -1 } else { 0 };
    for x in b.iter().take(8) {
        v = (v << 8) | i64::from(*x);
    }
    v
}

fn decode_oid(b: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    if let Some(first) = b.first() {
        out.push(u32::from(*first / 40));
        out.push(u32::from(*first % 40));
    }
    let mut v: u32 = 0;
    for x in b.iter().skip(1) {
        v = (v << 7) | u32::from(x & 0x7f);
        if x & 0x80 == 0 {
            out.push(v);
            v = 0;
        }
        if out.len() > 64 {
            break;
        }
    }
    out
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
    /// Where `b` starts in the whole message.
    base: usize,
}

impl<'a> Reader<'a> {
    /// The next element: its tag, its body, and where the body starts in the whole message.
    fn any(&mut self) -> Result<(u8, &'a [u8], usize)> {
        let t = || WireError::Truncated("snmp");
        let tag = *self.b.get(self.at).ok_or_else(t)?;
        let first = *self.b.get(self.at + 1).ok_or_else(t)?;
        let (len, head) = match first {
            n if n < 0x80 => (usize::from(n), 2),
            0x81 => (usize::from(*self.b.get(self.at + 2).ok_or_else(t)?), 3),
            0x82 => (usize::from(*self.b.get(self.at + 2).ok_or_else(t)?) << 8 | usize::from(*self.b.get(self.at + 3).ok_or_else(t)?), 4),
            _ => return Err(WireError::Unsupported("snmp", "length form".into())),
        };
        let body = self.b.get(self.at + head..self.at + head + len).ok_or_else(t)?;
        let start = self.base + self.at + head;
        self.at += head + len;
        Ok((tag, body, start))
    }

    fn take(&mut self, want: u8) -> Result<&'a [u8]> {
        Ok(self.take_at(want)?.0)
    }

    fn take_at(&mut self, want: u8) -> Result<(&'a [u8], usize)> {
        let (tag, body, start) = self.any()?;
        if tag != want {
            return Err(WireError::Unsupported("snmp", format!("tag 0x{tag:02x}")));
        }
        Ok((body, start))
    }

    fn enter(&mut self, want: u8) -> Result<Reader<'a>> {
        let (b, base) = self.take_at(want)?;
        Ok(Reader { b, at: 0, base })
    }

    fn integer(&mut self) -> Result<i64> {
        Ok(be_int(self.take(0x02)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let m = Snmp {
            community: "public".into(),
            kind: PduKind::Response,
            request_id: 4242,
            error_status: 0,
            error_index: 0,
            bindings: vec![
                (parse_oid("1.3.6.1.2.1.1.5.0").unwrap(), Value::Text("R1".into())),
                (parse_oid("1.3.6.1.2.1.1.3.0").unwrap(), Value::TimeTicks(123_456)),
                (parse_oid("1.3.6.1.2.1.2.2.1.8.300").unwrap(), Value::Integer(-2)),
                (parse_oid("1.3.6.1.2.1.1.2.0").unwrap(), Value::Oid(vec![1, 3, 6, 1, 4, 1, 99_999])),
                (parse_oid("1.3.6.1.2.1.1.9.0").unwrap(), Value::EndOfMib),
            ],
        };
        let b = m.encode();
        assert_eq!(Snmp::decode(&b).unwrap(), m);
        // The well-known encoding of 1.3.6.1.2.1.1.5.0.
        assert_eq!(oid_bytes(&[1, 3, 6, 1, 2, 1, 1, 5, 0]), vec![0x2b, 6, 1, 2, 1, 1, 5, 0]);
        for k in 0..b.len() {
            let _ = Snmp::decode(&b[..k]);
        }
        let long = Snmp { bindings: vec![(vec![1, 3, 6], Value::Text("x".repeat(300)))], ..m };
        assert_eq!(Snmp::decode(&long.encode()).unwrap(), long);
    }

    #[test]
    fn traps_round_trip() {
        let trap = Snmp {
            community: "avisos".into(),
            kind: PduKind::Trap,
            request_id: 9,
            error_status: 0,
            error_index: 0,
            bindings: vec![
                (oids::SYS_UPTIME.to_vec(), Value::TimeTicks(4_200)),
                (oids::SNMP_TRAP_OID.to_vec(), Value::Oid(oids::LINK_DOWN.to_vec())),
                ([oids::IF_INDEX, &[2]].concat(), Value::Integer(2)),
                (vec![1, 3, 6, 1, 6, 3, 15, 1, 1, 4, 0], Value::Counter(u32::MAX)),
            ],
        };
        let b = trap.encode();
        assert_eq!(version(&b), Some(1));
        assert_eq!(Snmp::decode(&b).unwrap(), trap);
        assert_eq!(oids::trap_name(oids::LINK_DOWN), Some("linkDown"));
        for k in 0..b.len() {
            let _ = Snmp::decode(&b[..k]);
            let _ = version(&b[..k]);
        }
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn rfc_3414_key_localization() {
        let engine = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
        assert_eq!(hex(&localize_key(Hash::Md5, "maplesyrup", &engine)), "526f5eed9fcce26f8964c2930787d82b");
        assert_eq!(hex(&localize_key(Hash::Sha1, "maplesyrup", &engine)), "6695febc9288e36282235fc7151f128497b38f3f");
        assert!(localize_key(Hash::Md5, "", &engine).is_empty());
    }

    #[test]
    fn v3_messages_round_trip_and_authenticate() {
        let pdu = Snmp {
            community: String::new(),
            kind: PduKind::Get,
            request_id: 77,
            error_status: 0,
            error_index: 0,
            bindings: vec![(oids::SYS_UPTIME.to_vec(), Value::Null)],
        };
        let m = V3 {
            msg_id: 5,
            flags: FLAG_REPORTABLE,
            engine_id: vec![0x80, 0, 0, 0, 3, 1, 2, 3, 4, 5, 6],
            boots: 1,
            time: 300,
            user: "ana".into(),
            auth: Vec::new(),
            context_name: String::new(),
            pdu,
        };
        let b = m.encode();
        assert_eq!(version(&b), Some(3));
        assert_eq!(V3::decode(&b).unwrap().0, m);
        assert!(Snmp::decode(&b).is_err(), "a v3 message isn't a community message");
        let key = localize_key(Hash::Sha1, "secreto123", &m.engine_id);
        let signed = m.encode_signed(Hash::Sha1, &key);
        let (back, _) = V3::decode(&signed).unwrap();
        assert_eq!(back.flags, FLAG_REPORTABLE | FLAG_AUTH);
        assert_eq!(back.auth.len(), AUTH_LEN);
        assert!(V3::verify(&signed, Hash::Sha1, &key));
        assert!(!V3::verify(&signed, Hash::Md5, &key));
        assert!(!V3::verify(&signed, Hash::Sha1, &localize_key(Hash::Sha1, "otraclave1", &m.engine_id)));
        assert!(!V3::verify(&b, Hash::Sha1, &key), "unauthenticated");
        let mut tampered = signed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(!V3::verify(&tampered, Hash::Sha1, &key));
        for k in 0..signed.len() {
            let _ = V3::decode(&signed[..k]);
            let _ = V3::verify(&signed[..k], Hash::Md5, &key);
        }
        // Privacy isn't supported: refused, not misread.
        let private = V3 { flags: FLAG_AUTH | FLAG_PRIV, ..m }.encode();
        assert!(V3::decode(&private).is_err());
    }
}
