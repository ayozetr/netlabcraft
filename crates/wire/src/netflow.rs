//! NetFlow export datagrams: version 5 (fixed 48-byte records) and version 9 (RFC 3954, a
//! template flowset followed by a data flowset).
//!
//! The exporter side always puts the template in front of the data it describes, so a collector
//! can read any datagram on its own; the decoder keeps no state between datagrams. Decoding is
//! bounded: at most [`MAX_RECORDS`] records per datagram, at most [`MAX_TEMPLATES`] templates of at
//! most [`MAX_FIELDS`] fields each, and every length is checked against the bytes present.

use std::net::Ipv4Addr;

use crate::{Result, WireError, be16, ip_at};

/// The port collectors usually listen on.
pub const NETFLOW_PORT: u16 = 2055;
/// Most records one version 5 datagram carries (the format's limit).
pub const MAX_RECORDS_V5: usize = 30;
/// Most records one version 9 datagram carries from this exporter.
pub const MAX_RECORDS_V9: usize = 24;
/// Most records the decoder accepts in one datagram.
pub const MAX_RECORDS: usize = 64;
const MAX_TEMPLATES: usize = 8;
const MAX_FIELDS: usize = 32;
const V5_HEADER: usize = 24;
const V5_RECORD: usize = 48;
const V9_HEADER: usize = 20;
/// The template id this exporter uses for its data flowsets.
const TEMPLATE_ID: u16 = 256;

/// The fields of the template this exporter sends: (field type, length in bytes).
const TEMPLATE: [(u16, u16); 14] =
    [(8, 4), (12, 4), (15, 4), (7, 2), (11, 2), (4, 1), (5, 1), (6, 1), (2, 4), (1, 4), (22, 4), (21, 4), (10, 2), (14, 2)];

/// One flow: what the exporter saw between two endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowRecord {
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub next_hop: Ipv4Addr,
    /// SNMP interface indexes the flow came in and went out on.
    pub input: u16,
    pub output: u16,
    pub packets: u32,
    pub octets: u32,
    /// System uptime in milliseconds at the first and the last packet.
    pub first: u32,
    pub last: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub tcp_flags: u8,
    pub protocol: u8,
    pub tos: u8,
}

/// A decoded export datagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    pub version: u16,
    pub uptime_ms: u32,
    pub unix_secs: u32,
    /// The flow sequence (version 5) or the package sequence (version 9).
    pub sequence: u32,
    pub records: Vec<FlowRecord>,
}

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// A version 5 datagram with the first [`MAX_RECORDS_V5`] of `records`.
pub fn encode_v5(uptime_ms: u32, unix_secs: u32, sequence: u32, records: &[FlowRecord]) -> Vec<u8> {
    let records = records.get(..records.len().min(MAX_RECORDS_V5)).unwrap_or(&[]);
    let mut v = Vec::with_capacity(V5_HEADER + V5_RECORD * records.len());
    put16(&mut v, 5);
    put16(&mut v, u16::try_from(records.len()).unwrap_or(0));
    put32(&mut v, uptime_ms);
    put32(&mut v, unix_secs);
    put32(&mut v, 0); // nanoseconds
    put32(&mut v, sequence);
    v.extend_from_slice(&[0, 0]); // engine type and id
    put16(&mut v, 0); // no sampling
    for r in records {
        v.extend_from_slice(&r.src.octets());
        v.extend_from_slice(&r.dst.octets());
        v.extend_from_slice(&r.next_hop.octets());
        put16(&mut v, r.input);
        put16(&mut v, r.output);
        put32(&mut v, r.packets);
        put32(&mut v, r.octets);
        put32(&mut v, r.first);
        put32(&mut v, r.last);
        put16(&mut v, r.src_port);
        put16(&mut v, r.dst_port);
        v.push(0);
        v.extend_from_slice(&[r.tcp_flags, r.protocol, r.tos]);
        put16(&mut v, 0); // source AS
        put16(&mut v, 0); // destination AS
        v.extend_from_slice(&[0, 0]); // masks
        put16(&mut v, 0);
    }
    v
}

fn put_value(v: &mut Vec<u8>, kind: u16, r: &FlowRecord) {
    match kind {
        8 => v.extend_from_slice(&r.src.octets()),
        12 => v.extend_from_slice(&r.dst.octets()),
        15 => v.extend_from_slice(&r.next_hop.octets()),
        7 => put16(v, r.src_port),
        11 => put16(v, r.dst_port),
        4 => v.push(r.protocol),
        5 => v.push(r.tos),
        6 => v.push(r.tcp_flags),
        2 => put32(v, r.packets),
        1 => put32(v, r.octets),
        22 => put32(v, r.first),
        21 => put32(v, r.last),
        10 => put16(v, r.input),
        14 => put16(v, r.output),
        _ => {}
    }
}

/// A version 9 datagram: the template, then the first [`MAX_RECORDS_V9`] of `records`.
pub fn encode_v9(uptime_ms: u32, unix_secs: u32, sequence: u32, source_id: u32, records: &[FlowRecord]) -> Vec<u8> {
    let records = records.get(..records.len().min(MAX_RECORDS_V9)).unwrap_or(&[]);
    let mut v = Vec::new();
    put16(&mut v, 9);
    put16(&mut v, u16::try_from(records.len() + 1).unwrap_or(1));
    put32(&mut v, uptime_ms);
    put32(&mut v, unix_secs);
    put32(&mut v, sequence);
    put32(&mut v, source_id);
    // Template flowset: id 0, length, template id, field count, the (type, length) pairs.
    put16(&mut v, 0);
    put16(&mut v, u16::try_from(8 + 4 * TEMPLATE.len()).unwrap_or(0));
    put16(&mut v, TEMPLATE_ID);
    put16(&mut v, u16::try_from(TEMPLATE.len()).unwrap_or(0));
    for (kind, len) in TEMPLATE {
        put16(&mut v, kind);
        put16(&mut v, len);
    }
    if !records.is_empty() {
        let start = v.len();
        put16(&mut v, TEMPLATE_ID);
        put16(&mut v, 0); // length, filled in below
        for r in records {
            for (kind, _) in TEMPLATE {
                put_value(&mut v, kind, r);
            }
        }
        while !(v.len() - start).is_multiple_of(4) {
            v.push(0);
        }
        let len = u16::try_from(v.len() - start).unwrap_or(0).to_be_bytes();
        if let Some(slot) = v.get_mut(start + 2..start + 4) {
            slot.copy_from_slice(&len);
        }
    }
    v
}

/// Reads a big-endian unsigned value of 1 to 8 bytes, saturated to 32 bits.
fn uint(b: &[u8], at: usize, len: usize) -> Option<u32> {
    if len == 0 || len > 8 {
        return None;
    }
    let s = b.get(at..at.checked_add(len)?)?;
    let mut x: u64 = 0;
    for byte in s {
        x = (x << 8) | u64::from(*byte);
    }
    Some(u32::try_from(x).unwrap_or(u32::MAX))
}

fn zero_record() -> FlowRecord {
    let z = Ipv4Addr::UNSPECIFIED;
    FlowRecord {
        src: z,
        dst: z,
        next_hop: z,
        input: 0,
        output: 0,
        packets: 0,
        octets: 0,
        first: 0,
        last: 0,
        src_port: 0,
        dst_port: 0,
        tcp_flags: 0,
        protocol: 0,
        tos: 0,
    }
}

/// Decodes a version 5 or version 9 export datagram.
pub fn decode(b: &[u8]) -> Result<Datagram> {
    match be16(b, 0).ok_or(WireError::Truncated("netflow"))? {
        5 => decode_v5(b),
        9 => decode_v9(b),
        v => Err(WireError::Unsupported("netflow", format!("version {v}"))),
    }
}

fn decode_v5(b: &[u8]) -> Result<Datagram> {
    let trunc = || WireError::Truncated("netflow v5");
    let count = usize::from(be16(b, 2).ok_or_else(trunc)?);
    if count > MAX_RECORDS_V5 {
        return Err(WireError::Unsupported("netflow v5", format!("{count} records")));
    }
    let uptime_ms = be32(b, 4).ok_or_else(trunc)?;
    let unix_secs = be32(b, 8).ok_or_else(trunc)?;
    let sequence = be32(b, 16).ok_or_else(trunc)?;
    if b.len() < V5_HEADER + count * V5_RECORD {
        return Err(trunc());
    }
    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let at = V5_HEADER + i * V5_RECORD;
        let r = FlowRecord {
            src: ip_at(b, at).ok_or_else(trunc)?,
            dst: ip_at(b, at + 4).ok_or_else(trunc)?,
            next_hop: ip_at(b, at + 8).ok_or_else(trunc)?,
            input: be16(b, at + 12).ok_or_else(trunc)?,
            output: be16(b, at + 14).ok_or_else(trunc)?,
            packets: be32(b, at + 16).ok_or_else(trunc)?,
            octets: be32(b, at + 20).ok_or_else(trunc)?,
            first: be32(b, at + 24).ok_or_else(trunc)?,
            last: be32(b, at + 28).ok_or_else(trunc)?,
            src_port: be16(b, at + 32).ok_or_else(trunc)?,
            dst_port: be16(b, at + 34).ok_or_else(trunc)?,
            tcp_flags: *b.get(at + 37).ok_or_else(trunc)?,
            protocol: *b.get(at + 38).ok_or_else(trunc)?,
            tos: *b.get(at + 39).ok_or_else(trunc)?,
        };
        records.push(r);
    }
    Ok(Datagram { version: 5, uptime_ms, unix_secs, sequence, records })
}

fn decode_v9(b: &[u8]) -> Result<Datagram> {
    let trunc = || WireError::Truncated("netflow v9");
    let uptime_ms = be32(b, 4).ok_or_else(trunc)?;
    let unix_secs = be32(b, 8).ok_or_else(trunc)?;
    let sequence = be32(b, 12).ok_or_else(trunc)?;
    if b.len() < V9_HEADER {
        return Err(trunc());
    }
    let mut templates: Vec<(u16, Vec<(u16, u16)>)> = Vec::new();
    let mut records = Vec::new();
    let mut at = V9_HEADER;
    while at + 4 <= b.len() {
        let id = be16(b, at).ok_or_else(trunc)?;
        let len = usize::from(be16(b, at + 2).ok_or_else(trunc)?);
        if len < 4 || at + len > b.len() {
            return Err(trunc());
        }
        let body = at + 4;
        let end = at + len;
        if id == 0 {
            // Template records: id, field count, the pairs; more than one may follow.
            let mut p = body;
            while p + 4 <= end {
                let tid = be16(b, p).ok_or_else(trunc)?;
                let n = usize::from(be16(b, p + 2).ok_or_else(trunc)?);
                if n > MAX_FIELDS || p + 4 + n * 4 > end {
                    return Err(trunc());
                }
                let mut fields = Vec::with_capacity(n);
                for i in 0..n {
                    let q = p + 4 + i * 4;
                    fields.push((be16(b, q).ok_or_else(trunc)?, be16(b, q + 2).ok_or_else(trunc)?));
                }
                if templates.len() < MAX_TEMPLATES {
                    templates.push((tid, fields));
                }
                p += 4 + n * 4;
            }
        } else if id >= 256
            && let Some((_, fields)) = templates.iter().find(|(t, _)| *t == id)
        {
            let size: usize = fields.iter().map(|(_, l)| usize::from(*l)).sum();
            if size > 0 {
                let mut p = body;
                while p + size <= end && records.len() < MAX_RECORDS {
                    let mut r = zero_record();
                    let mut q = p;
                    for (kind, l) in fields {
                        let l = usize::from(*l);
                        let x = uint(b, q, l);
                        match (*kind, x) {
                            (8, Some(x)) if l == 4 => r.src = Ipv4Addr::from(x),
                            (12, Some(x)) if l == 4 => r.dst = Ipv4Addr::from(x),
                            (15, Some(x)) if l == 4 => r.next_hop = Ipv4Addr::from(x),
                            (7, Some(x)) => r.src_port = u16::try_from(x).unwrap_or(u16::MAX),
                            (11, Some(x)) => r.dst_port = u16::try_from(x).unwrap_or(u16::MAX),
                            (4, Some(x)) => r.protocol = u8::try_from(x).unwrap_or(u8::MAX),
                            (5, Some(x)) => r.tos = u8::try_from(x).unwrap_or(u8::MAX),
                            (6, Some(x)) => r.tcp_flags = u8::try_from(x).unwrap_or(u8::MAX),
                            (2, Some(x)) => r.packets = x,
                            (1, Some(x)) => r.octets = x,
                            (22, Some(x)) => r.first = x,
                            (21, Some(x)) => r.last = x,
                            (10, Some(x)) => r.input = u16::try_from(x).unwrap_or(u16::MAX),
                            (14, Some(x)) => r.output = u16::try_from(x).unwrap_or(u16::MAX),
                            _ => {}
                        }
                        q += l;
                    }
                    records.push(r);
                    p += size;
                }
            }
        }
        at = end;
    }
    Ok(Datagram { version: 9, uptime_ms, unix_secs, sequence, records })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(n: u8) -> FlowRecord {
        FlowRecord {
            src: Ipv4Addr::new(10, 0, 0, n),
            dst: Ipv4Addr::new(192, 168, 1, n),
            next_hop: Ipv4Addr::new(10, 1, 1, 1),
            input: 1,
            output: 2,
            packets: 3 + u32::from(n),
            octets: 1500 * u32::from(n),
            first: 100,
            last: 900,
            src_port: 40_000 + u16::from(n),
            dst_port: 80,
            tcp_flags: 0x12,
            protocol: 6,
            tos: 0,
        }
    }

    #[test]
    fn version_5_has_the_published_layout_and_round_trips() {
        let records: Vec<FlowRecord> = (1..=3).map(rec).collect();
        let b = encode_v5(5_000, 1_800_000_000, 77, &records);
        assert_eq!(b.len(), 24 + 3 * 48);
        assert_eq!(&b[..4], &[0, 5, 0, 3]);
        assert_eq!(u32::from_be_bytes([b[16], b[17], b[18], b[19]]), 77);
        // The first record: addresses, then the packet count at offset 16 of the record.
        assert_eq!(&b[24..28], &[10, 0, 0, 1]);
        assert_eq!(u32::from_be_bytes([b[40], b[41], b[42], b[43]]), 4);
        // Protocol is the 39th byte of a record, the TCP flags the 38th.
        assert_eq!(b[24 + 37], 0x12);
        assert_eq!(b[24 + 38], 6);
        let d = decode(&b).unwrap();
        assert_eq!(d.version, 5);
        assert_eq!(d.sequence, 77);
        assert_eq!(d.uptime_ms, 5_000);
        assert_eq!(d.records, records);
    }

    #[test]
    fn version_5_carries_at_most_thirty_records() {
        let records: Vec<FlowRecord> = (0..40).map(rec).collect();
        let b = encode_v5(0, 0, 0, &records);
        assert_eq!(decode(&b).unwrap().records.len(), MAX_RECORDS_V5);
        // A count above the format's limit is refused.
        let mut bad = b.clone();
        bad[3] = 31;
        assert!(decode(&bad).is_err());
    }

    #[test]
    fn version_9_carries_its_template_and_round_trips() {
        let records: Vec<FlowRecord> = (1..=5).map(rec).collect();
        let b = encode_v9(7_000, 1_800_000_001, 9, 42, &records);
        assert_eq!(&b[..2], &[0, 9]);
        assert!(b.len().is_multiple_of(4));
        let d = decode(&b).unwrap();
        assert_eq!(d.version, 9);
        assert_eq!(d.sequence, 9);
        assert_eq!(d.records, records);
        // No records: the template alone.
        assert!(decode(&encode_v9(0, 0, 0, 0, &[])).unwrap().records.is_empty());
    }

    #[test]
    fn hostile_input_is_refused_not_trusted() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[0, 7, 0, 0]).is_err());
        for b in [encode_v5(1, 2, 3, &[rec(1), rec(2)]), encode_v9(1, 2, 3, 4, &[rec(1), rec(2)])] {
            for n in 0..b.len() {
                let _ = decode(&b[..n]);
            }
            // Every single-byte change decodes or errors, never panics.
            for i in 0..b.len() {
                let mut c = b.clone();
                c[i] ^= 0xff;
                let _ = decode(&c);
            }
        }
    }
}
