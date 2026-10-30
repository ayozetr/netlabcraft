//! SNMPv2c (RFC 3416, BER-encoded as in RFC 1157): GetRequest, GetNextRequest, SetRequest and
//! the Response, with the community string.

use crate::{Result, WireError};

pub const SNMP_PORT: u16 = 161;

/// A value in a variable binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Integer(i64),
    Text(String),
    Oid(Vec<u32>),
    /// Hundredths of a second.
    TimeTicks(u32),
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
}

impl PduKind {
    fn tag(self) -> u8 {
        match self {
            PduKind::Get => 0xa0,
            PduKind::GetNext => 0xa1,
            PduKind::Response => 0xa2,
            PduKind::Set => 0xa3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PduKind::Get => "get-request",
            PduKind::GetNext => "get-next-request",
            PduKind::Response => "response",
            PduKind::Set => "set-request",
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
    pub fn encode(&self) -> Vec<u8> {
        let mut binds = Vec::new();
        for (oid, v) in &self.bindings {
            binds.extend(tlv(0x30, &[tlv(0x06, &oid_bytes(oid)), v.encode()].concat()));
        }
        let pdu = [
            tlv(0x02, &int(i64::from(self.request_id))),
            tlv(0x02, &int(i64::from(self.error_status))),
            tlv(0x02, &int(i64::from(self.error_index))),
            tlv(0x30, &binds),
        ]
        .concat();
        // Version 1 means SNMPv2c.
        tlv(0x30, &[tlv(0x02, &[1]), tlv(0x04, self.community.as_bytes()), tlv(self.kind.tag(), &pdu)].concat())
    }

    pub fn decode(b: &[u8]) -> Result<Snmp> {
        let mut r = Reader { b, at: 0 };
        let mut msg = r.enter(0x30)?;
        let _version = msg.integer()?;
        let community = String::from_utf8_lossy(msg.take(0x04)?).into_owned();
        let tag = *msg.b.get(msg.at).ok_or(WireError::Truncated("snmp"))?;
        let kind = match tag {
            0xa0 => PduKind::Get,
            0xa1 => PduKind::GetNext,
            0xa2 => PduKind::Response,
            0xa3 => PduKind::Set,
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
            let (t, body) = vb.any()?;
            let value = match t {
                0x05 => Value::Null,
                0x02 => Value::Integer(be_int(body)),
                0x04 => Value::Text(String::from_utf8_lossy(body).into_owned()),
                0x06 => Value::Oid(decode_oid(body)),
                0x43 => Value::TimeTicks(u32::try_from(be_int(body)).unwrap_or(0)),
                0x82 => Value::EndOfMib,
                _ => Value::NoSuchObject,
            };
            bindings.push((oid, value));
        }
        Ok(Snmp { community, kind, request_id, error_status, error_index, bindings })
    }
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
}

impl<'a> Reader<'a> {
    fn any(&mut self) -> Result<(u8, &'a [u8])> {
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
        self.at += head + len;
        Ok((tag, body))
    }

    fn take(&mut self, want: u8) -> Result<&'a [u8]> {
        let (tag, body) = self.any()?;
        if tag != want {
            return Err(WireError::Unsupported("snmp", format!("tag 0x{tag:02x}")));
        }
        Ok(body)
    }

    fn enter(&mut self, want: u8) -> Result<Reader<'a>> {
        Ok(Reader { b: self.take(want)?, at: 0 })
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
}
