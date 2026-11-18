//! PIM version 2 (RFC 7761), IPv4: the hellos routers use to find each other, the join and
//! prune messages that build a multicast tree, and the register messages a source's first router
//! sends to the rendezvous point.
//!
//! Bootstrap, assert and graft messages are not decoded: they come back as `Unsupported`.

use std::net::Ipv4Addr;

use crate::{Result, WireError, be16, checksum, ip_at};

/// PIM's IP protocol number.
pub const PROTO_PIM: u8 = 103;
/// The group every PIM router listens to (hellos and joins go there).
pub const ALL_PIM_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 13);

/// How often a router sends a hello (RFC 7761 §4.11), in seconds.
pub const HELLO_PERIOD_SECS: u16 = 30;
/// A neighbour is held for this long after its last hello, in seconds (3.5 times the period).
pub const HELLO_HOLDTIME_SECS: u16 = 105;
/// How often a router repeats its joins, in seconds.
pub const JOIN_PRUNE_PERIOD_SECS: u16 = 60;
/// A join is held for this long, in seconds (3.5 times the period).
pub const JOIN_PRUNE_HOLDTIME_SECS: u16 = 210;
/// The DR priority of a router that was not told another.
pub const DEFAULT_DR_PRIORITY: u32 = 1;

/// The most groups, and the most sources in one group, a join/prune message may carry here.
pub const MAX_GROUPS: usize = 64;
pub const MAX_SOURCES: usize = 256;

const VERSION: u8 = 2;
const TYPE_HELLO: u8 = 0;
const TYPE_REGISTER: u8 = 1;
const TYPE_REGISTER_STOP: u8 = 2;
const TYPE_JOIN_PRUNE: u8 = 3;

const OPT_HOLDTIME: u16 = 1;
const OPT_LAN_PRUNE_DELAY: u16 = 2;
const OPT_DR_PRIORITY: u16 = 19;
const OPT_GENERATION_ID: u16 = 20;

const FAMILY_IPV4: u8 = 1;
const SRC_SPARSE: u8 = 0x04;
const SRC_WILDCARD: u8 = 0x02;
const SRC_RPT: u8 = 0x01;
const REG_BORDER: u8 = 0x80;
const REG_NULL: u8 = 0x40;

/// A hello (§4.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hello {
    /// Seconds the sender is to be held as a neighbour (0xffff: never expires, 0: gone).
    pub holdtime: u16,
    /// The sender's priority for becoming the designated router.
    pub dr_priority: Option<u32>,
    /// A number the sender picks again every time it restarts.
    pub generation_id: Option<u32>,
}

impl Hello {
    pub fn new(dr_priority: u32, generation_id: u32) -> Hello {
        Hello { holdtime: HELLO_HOLDTIME_SECS, dr_priority: Some(dr_priority), generation_id: Some(generation_id) }
    }
}

/// One source (or the rendezvous point) in a join/prune group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpSource {
    pub addr: Ipv4Addr,
    /// The wildcard bit: the entry is (*,G) and `addr` is the rendezvous point.
    pub wildcard: bool,
    /// The shared-tree bit: the entry concerns the shared tree.
    pub rpt: bool,
}

impl JpSource {
    /// A (*,G) entry for the rendezvous point `rp`.
    pub fn star(rp: Ipv4Addr) -> JpSource {
        JpSource { addr: rp, wildcard: true, rpt: true }
    }

    /// An (S,G) entry on the shortest-path tree.
    pub fn source(s: Ipv4Addr) -> JpSource {
        JpSource { addr: s, wildcard: false, rpt: false }
    }

    /// An (S,G,rpt) entry: a source cut from the shared tree.
    pub fn source_rpt(s: Ipv4Addr) -> JpSource {
        JpSource { addr: s, wildcard: false, rpt: true }
    }
}

/// The joins and prunes for one group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpGroup {
    pub group: Ipv4Addr,
    pub joins: Vec<JpSource>,
    pub prunes: Vec<JpSource>,
}

/// A join/prune message (§4.5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinPrune {
    /// The neighbour the message is for; other routers on the link only listen.
    pub upstream: Ipv4Addr,
    /// Seconds the joins are held.
    pub holdtime: u16,
    pub groups: Vec<JpGroup>,
}

/// One PIM message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pim {
    Hello(Hello),
    /// A source's first router sends the source's packets to the rendezvous point in this
    /// (§4.4.1); a null register carries a header only.
    Register {
        border: bool,
        null: bool,
        data: Vec<u8>,
    },
    RegisterStop {
        group: Ipv4Addr,
        source: Ipv4Addr,
    },
    JoinPrune(JoinPrune),
}

fn unicast(v: &mut Vec<u8>, a: Ipv4Addr) {
    v.extend_from_slice(&[FAMILY_IPV4, 0]);
    v.extend_from_slice(&a.octets());
}

fn group(v: &mut Vec<u8>, a: Ipv4Addr) {
    v.extend_from_slice(&[FAMILY_IPV4, 0, 0, 32]);
    v.extend_from_slice(&a.octets());
}

fn source(v: &mut Vec<u8>, s: &JpSource) {
    let flags = SRC_SPARSE | if s.wildcard { SRC_WILDCARD } else { 0 } | if s.rpt { SRC_RPT } else { 0 };
    v.extend_from_slice(&[FAMILY_IPV4, 0, flags, 32]);
    v.extend_from_slice(&s.addr.octets());
}

fn read_unicast(b: &[u8], at: usize) -> Result<Ipv4Addr> {
    let t = || WireError::Truncated("pim");
    if b.get(at).copied().ok_or_else(t)? != FAMILY_IPV4 || b.get(at + 1).copied().ok_or_else(t)? != 0 {
        return Err(WireError::Unsupported("pim", "an address that is not IPv4".into()));
    }
    ip_at(b, at + 2).ok_or_else(t)
}

fn read_group(b: &[u8], at: usize) -> Result<Ipv4Addr> {
    let t = || WireError::Truncated("pim");
    if b.get(at).copied().ok_or_else(t)? != FAMILY_IPV4 || b.get(at + 1).copied().ok_or_else(t)? != 0 {
        return Err(WireError::Unsupported("pim", "a group that is not IPv4".into()));
    }
    ip_at(b, at + 4).ok_or_else(t)
}

fn read_source(b: &[u8], at: usize) -> Result<JpSource> {
    let t = || WireError::Truncated("pim");
    if b.get(at).copied().ok_or_else(t)? != FAMILY_IPV4 || b.get(at + 1).copied().ok_or_else(t)? != 0 {
        return Err(WireError::Unsupported("pim", "a source that is not IPv4".into()));
    }
    let flags = b.get(at + 2).copied().ok_or_else(t)?;
    Ok(JpSource { addr: ip_at(b, at + 4).ok_or_else(t)?, wildcard: flags & SRC_WILDCARD != 0, rpt: flags & SRC_RPT != 0 })
}

fn option(v: &mut Vec<u8>, kind: u16, value: &[u8]) {
    v.extend_from_slice(&kind.to_be_bytes());
    v.extend_from_slice(&u16::try_from(value.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(value);
}

impl Pim {
    /// The message as it goes in an IP packet of protocol 103, with its checksum.
    pub fn encode(&self) -> Vec<u8> {
        let (ty, mut body) = match self {
            Pim::Hello(h) => {
                let mut v = Vec::new();
                option(&mut v, OPT_HOLDTIME, &h.holdtime.to_be_bytes());
                // The prune-delay option routers send by default: 500 ms, override 2.5 s.
                option(&mut v, OPT_LAN_PRUNE_DELAY, &[0x01, 0xf4, 0x09, 0xc4]);
                if let Some(p) = h.dr_priority {
                    option(&mut v, OPT_DR_PRIORITY, &p.to_be_bytes());
                }
                if let Some(g) = h.generation_id {
                    option(&mut v, OPT_GENERATION_ID, &g.to_be_bytes());
                }
                (TYPE_HELLO, v)
            }
            Pim::Register { border, null, data } => {
                let flags = if *border { REG_BORDER } else { 0 } | if *null { REG_NULL } else { 0 };
                let mut v = vec![flags, 0, 0, 0];
                v.extend_from_slice(data);
                (TYPE_REGISTER, v)
            }
            Pim::RegisterStop { group: g, source: s } => {
                let mut v = Vec::new();
                group(&mut v, *g);
                unicast(&mut v, *s);
                (TYPE_REGISTER_STOP, v)
            }
            Pim::JoinPrune(jp) => {
                let mut v = Vec::new();
                unicast(&mut v, jp.upstream);
                v.push(0);
                v.push(u8::try_from(jp.groups.len()).unwrap_or(u8::MAX));
                v.extend_from_slice(&jp.holdtime.to_be_bytes());
                for g in jp.groups.iter().take(usize::from(u8::MAX)) {
                    group(&mut v, g.group);
                    v.extend_from_slice(&u16::try_from(g.joins.len()).unwrap_or(u16::MAX).to_be_bytes());
                    v.extend_from_slice(&u16::try_from(g.prunes.len()).unwrap_or(u16::MAX).to_be_bytes());
                    for s in g.joins.iter().chain(&g.prunes) {
                        source(&mut v, s);
                    }
                }
                (TYPE_JOIN_PRUNE, v)
            }
        };
        let mut out = vec![(VERSION << 4) | ty, 0, 0, 0];
        out.append(&mut body);
        // A register's checksum covers the header and the flags only, not the packet it carries.
        let covered = if ty == TYPE_REGISTER { out.len().min(8) } else { out.len() };
        let sum = checksum(&[out.get(..covered).unwrap_or(&[])]);
        out[2..4].copy_from_slice(&sum.to_be_bytes());
        out
    }

    /// Decodes a message, checking its version and checksum.
    pub fn decode(b: &[u8]) -> Result<Pim> {
        let t = || WireError::Truncated("pim");
        let first = b.first().copied().ok_or_else(t)?;
        if b.len() < 4 {
            return Err(t());
        }
        if first >> 4 != VERSION {
            return Err(WireError::Unsupported("pim", format!("version {}", first >> 4)));
        }
        let ty = first & 0x0f;
        let covered = if ty == TYPE_REGISTER { b.len().min(8) } else { b.len() };
        if checksum(&[b.get(..covered).ok_or_else(t)?]) != 0 {
            return Err(WireError::Checksum("pim"));
        }
        let body = b.get(4..).ok_or_else(t)?;
        match ty {
            TYPE_HELLO => {
                let mut h = Hello { holdtime: HELLO_HOLDTIME_SECS, dr_priority: None, generation_id: None };
                let mut at = 0;
                while at < body.len() {
                    let kind = be16(body, at).ok_or_else(t)?;
                    let len = usize::from(be16(body, at + 2).ok_or_else(t)?);
                    let val = body.get(at + 4..at + 4 + len).ok_or_else(t)?;
                    let word = || -> Option<u32> { val.get(..4).and_then(|s| <[u8; 4]>::try_from(s).ok()).map(u32::from_be_bytes) };
                    match kind {
                        OPT_HOLDTIME => h.holdtime = be16(val, 0).ok_or_else(t)?,
                        OPT_DR_PRIORITY => h.dr_priority = Some(word().ok_or_else(t)?),
                        OPT_GENERATION_ID => h.generation_id = Some(word().ok_or_else(t)?),
                        _ => {}
                    }
                    at += 4 + len;
                }
                Ok(Pim::Hello(h))
            }
            TYPE_REGISTER => {
                let flags = body.first().copied().ok_or_else(t)?;
                let data = body.get(4..).ok_or_else(t)?.to_vec();
                Ok(Pim::Register { border: flags & REG_BORDER != 0, null: flags & REG_NULL != 0, data })
            }
            TYPE_REGISTER_STOP => Ok(Pim::RegisterStop { group: read_group(body, 0)?, source: read_unicast(body, 8)? }),
            TYPE_JOIN_PRUNE => {
                let upstream = read_unicast(body, 0)?;
                let n_groups = usize::from(body.get(7).copied().ok_or_else(t)?);
                let holdtime = be16(body, 8).ok_or_else(t)?;
                if n_groups > MAX_GROUPS {
                    return Err(WireError::Unsupported("pim", format!("{n_groups} groups in one message")));
                }
                let mut at = 10;
                let mut groups = Vec::new();
                for _ in 0..n_groups {
                    let g = read_group(body, at)?;
                    let nj = usize::from(be16(body, at + 8).ok_or_else(t)?);
                    let np = usize::from(be16(body, at + 10).ok_or_else(t)?);
                    if nj > MAX_SOURCES || np > MAX_SOURCES {
                        return Err(WireError::Unsupported("pim", format!("{} sources in one group", nj + np)));
                    }
                    at += 12;
                    let mut joins = Vec::new();
                    let mut prunes = Vec::new();
                    for i in 0..nj + np {
                        let s = read_source(body, at)?;
                        at += 8;
                        if i < nj { joins.push(s) } else { prunes.push(s) }
                    }
                    groups.push(JpGroup { group: g, joins, prunes });
                }
                Ok(Pim::JoinPrune(JoinPrune { upstream, holdtime, groups }))
            }
            other => Err(WireError::Unsupported("pim", format!("type {other}"))),
        }
    }

    /// One line for a capture.
    pub fn describe(&self) -> String {
        match self {
            Pim::Hello(h) => format!("PIM hello (holdtime {} s)", h.holdtime),
            Pim::Register { null: true, .. } => "PIM null register".to_string(),
            Pim::Register { .. } => "PIM register".to_string(),
            Pim::RegisterStop { group, source } => format!("PIM register-stop for ({source}, {group})"),
            Pim::JoinPrune(jp) => {
                let joins: usize = jp.groups.iter().map(|g| g.joins.len()).sum();
                let prunes: usize = jp.groups.iter().map(|g| g.prunes.len()).sum();
                format!(
                    "PIM join/prune to {} ({joins} join{}, {prunes} prune{})",
                    jp.upstream,
                    if joins == 1 { "" } else { "s" },
                    if prunes == 1 { "" } else { "s" }
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jp() -> Pim {
        Pim::JoinPrune(JoinPrune {
            upstream: Ipv4Addr::new(10, 0, 0, 1),
            holdtime: JOIN_PRUNE_HOLDTIME_SECS,
            groups: vec![
                JpGroup {
                    group: Ipv4Addr::new(239, 1, 1, 1),
                    joins: vec![JpSource::star(Ipv4Addr::new(1, 1, 1, 1)), JpSource::source(Ipv4Addr::new(10, 1, 1, 10))],
                    prunes: vec![JpSource::source_rpt(Ipv4Addr::new(10, 1, 1, 11))],
                },
                JpGroup { group: Ipv4Addr::new(239, 2, 2, 2), joins: vec![], prunes: vec![JpSource::source(Ipv4Addr::new(10, 1, 1, 12))] },
            ],
        })
    }

    fn all() -> Vec<Pim> {
        vec![
            Pim::Hello(Hello::new(7, 0xdead_beef)),
            Pim::Hello(Hello { holdtime: 0, dr_priority: None, generation_id: None }),
            jp(),
            Pim::RegisterStop { group: Ipv4Addr::new(239, 1, 1, 1), source: Ipv4Addr::new(10, 1, 1, 10) },
        ]
    }

    #[test]
    fn messages_round_trip_with_a_valid_checksum() {
        for m in all() {
            let v = m.encode();
            assert_eq!(v.first().map(|b| b >> 4), Some(2));
            assert_eq!(checksum(&[&v]), 0, "{m:?}");
            assert_eq!(Pim::decode(&v).unwrap(), m);
        }
    }

    #[test]
    fn a_register_checksum_leaves_out_the_packet_it_carries() {
        let m = Pim::Register { border: false, null: false, data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] };
        let v = m.encode();
        assert_eq!(checksum(&[&v[..8]]), 0);
        assert_eq!(Pim::decode(&v).unwrap(), m);
        // The carried packet may change without the checksum noticing.
        let mut other = v.clone();
        other[12] ^= 0xff;
        assert!(Pim::decode(&other).is_ok());
        let null = Pim::Register { border: true, null: true, data: Vec::new() };
        assert_eq!(Pim::decode(&null.encode()).unwrap(), null);
        assert!(null.describe().contains("null"));
    }

    #[test]
    fn the_source_flags_are_on_the_wire() {
        let v = jp().encode();
        // Header 4, upstream 6, reserved 1, count 1, holdtime 2, group 8, two counts 4: sources start at 26.
        assert_eq!(v[26 + 2], 0x07);
        assert_eq!(v[34 + 2], 0x04);
        assert_eq!(v[42 + 2], 0x05);
    }

    #[test]
    fn unknown_hello_options_are_skipped() {
        let mut v = vec![0x20, 0, 0, 0];
        v.extend_from_slice(&[0x00, 0x01, 0x00, 0x02, 0x00, 0x69]);
        v.extend_from_slice(&[0x00, 0x63, 0x00, 0x03, 1, 2, 3]);
        let sum = checksum(&[&v]);
        v[2..4].copy_from_slice(&sum.to_be_bytes());
        assert_eq!(Pim::decode(&v).unwrap(), Pim::Hello(Hello { holdtime: 105, dr_priority: None, generation_id: None }));
    }

    #[test]
    fn truncated_and_corrupt_messages_are_errors() {
        for m in all() {
            let v = m.encode();
            for n in 0..v.len() {
                assert!(Pim::decode(&v[..n]).is_err(), "{m:?} cut at {n}");
            }
            let mut bad = v.clone();
            let last = bad.len() - 1;
            bad[last] ^= 1;
            assert!(Pim::decode(&bad).is_err());
        }
        let mut other = Pim::Hello(Hello::new(1, 1)).encode();
        other[0] = 0x35;
        assert!(Pim::decode(&other).is_err());
        let mut v1 = Pim::Hello(Hello::new(1, 1)).encode();
        v1[0] = 0x10;
        assert!(Pim::decode(&v1).is_err());
    }

    #[test]
    fn a_message_that_lies_about_its_size_is_refused() {
        // 255 groups in a message that carries none.
        let mut v = vec![0x23, 0, 0, 0];
        v.extend_from_slice(&[1, 0, 10, 0, 0, 1]);
        v.extend_from_slice(&[0, 255, 0, 210]);
        let sum = checksum(&[&v]);
        v[2..4].copy_from_slice(&sum.to_be_bytes());
        assert!(Pim::decode(&v).is_err());
    }

    #[test]
    fn describe_names_what_each_message_is() {
        assert!(Pim::Hello(Hello::new(1, 1)).describe().contains("hello"));
        assert!(jp().describe().contains("2 joins, 2 prunes"));
        assert!(Pim::RegisterStop { group: Ipv4Addr::new(239, 1, 1, 1), source: Ipv4Addr::new(1, 1, 1, 1) }.describe().contains("register-stop"));
    }
}
