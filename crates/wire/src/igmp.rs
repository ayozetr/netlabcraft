//! IGMP version 2 (RFC 2236): the membership queries, reports and leave messages that hosts and
//! queriers exchange, which an IGMP-snooping switch listens to.

use std::net::Ipv4Addr;

use crate::{Result, WireError, checksum, ip_at};

/// IGMP's IP protocol number.
pub const PROTO_IGMP: u8 = 2;
/// The group every host listens to (general queries go there).
pub const ALL_HOSTS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 1);
/// The group routers listen to (leave messages go there).
pub const ALL_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 2);

const TYPE_QUERY: u8 = 0x11;
const TYPE_REPORT_V1: u8 = 0x12;
const TYPE_REPORT_V2: u8 = 0x16;
const TYPE_LEAVE: u8 = 0x17;

/// What an IGMP message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgmpKind {
    /// A membership query: general (group 0.0.0.0) or for one group.
    Query,
    /// A version 1 membership report.
    ReportV1,
    /// A version 2 membership report.
    ReportV2,
    /// A host leaves a group.
    Leave,
}

/// One IGMPv2 message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Igmp {
    pub kind: IgmpKind,
    /// Longest wait before answering a query, in tenths of a second (0 for the others).
    pub max_resp: u8,
    /// The group (0.0.0.0 in a general query).
    pub group: Ipv4Addr,
}

impl Igmp {
    /// A general query asking every host for its groups.
    pub fn general_query(max_resp: u8) -> Igmp {
        Igmp { kind: IgmpKind::Query, max_resp, group: Ipv4Addr::UNSPECIFIED }
    }

    pub fn report(group: Ipv4Addr) -> Igmp {
        Igmp { kind: IgmpKind::ReportV2, max_resp: 0, group }
    }

    pub fn leave(group: Ipv4Addr) -> Igmp {
        Igmp { kind: IgmpKind::Leave, max_resp: 0, group }
    }

    pub fn encode(&self) -> Vec<u8> {
        let ty = match self.kind {
            IgmpKind::Query => TYPE_QUERY,
            IgmpKind::ReportV1 => TYPE_REPORT_V1,
            IgmpKind::ReportV2 => TYPE_REPORT_V2,
            IgmpKind::Leave => TYPE_LEAVE,
        };
        let mut v = vec![ty, self.max_resp, 0, 0];
        v.extend_from_slice(&self.group.octets());
        let sum = checksum(&[&v]);
        v[2..4].copy_from_slice(&sum.to_be_bytes());
        v
    }

    /// Decodes a message (a version 3 query's extra fields are ignored).
    pub fn decode(b: &[u8]) -> Result<Igmp> {
        let t = || WireError::Truncated("igmp");
        let head = b.get(..8).ok_or_else(t)?;
        if checksum(&[b]) != 0 {
            return Err(WireError::Checksum("igmp"));
        }
        let kind = match head.first().copied().ok_or_else(t)? {
            TYPE_QUERY => IgmpKind::Query,
            TYPE_REPORT_V1 => IgmpKind::ReportV1,
            TYPE_REPORT_V2 => IgmpKind::ReportV2,
            TYPE_LEAVE => IgmpKind::Leave,
            other => return Err(WireError::Unsupported("igmp", format!("type 0x{other:02x}"))),
        };
        Ok(Igmp { kind, max_resp: head.get(1).copied().ok_or_else(t)?, group: ip_at(b, 4).ok_or_else(t)? })
    }

    pub fn describe(&self) -> String {
        match self.kind {
            IgmpKind::Query if self.group.is_unspecified() => "IGMP general membership query".to_string(),
            IgmpKind::Query => format!("IGMP membership query for {}", self.group),
            IgmpKind::ReportV1 | IgmpKind::ReportV2 => format!("IGMP membership report for {}", self.group),
            IgmpKind::Leave => format!("IGMP leave for {}", self.group),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_with_a_valid_checksum() {
        for m in [Igmp::general_query(100), Igmp::report(Ipv4Addr::new(239, 1, 1, 1)), Igmp::leave(Ipv4Addr::new(239, 1, 1, 1))] {
            let v = m.encode();
            assert_eq!(v.len(), 8);
            assert_eq!(checksum(&[&v]), 0);
            assert_eq!(Igmp::decode(&v).unwrap(), m);
        }
        assert_eq!(Igmp::report(Ipv4Addr::new(239, 1, 1, 1)).encode()[0], 0x16);
        assert!(Igmp::general_query(100).describe().contains("general"));
    }

    #[test]
    fn truncated_and_corrupt_messages_are_errors() {
        let v = Igmp::report(Ipv4Addr::new(239, 1, 1, 1)).encode();
        for n in 0..v.len() {
            assert!(Igmp::decode(&v[..n]).is_err(), "{n}");
        }
        let mut bad = v.clone();
        bad[7] ^= 1;
        assert!(Igmp::decode(&bad).is_err());
        let mut other = Igmp::report(Ipv4Addr::new(239, 1, 1, 1)).encode();
        other[0] = 0x99;
        assert!(Igmp::decode(&other).is_err());
    }
}
