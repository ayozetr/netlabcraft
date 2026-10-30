//! LLDP (IEEE 802.1AB): each port tells its neighbour who it is, in type-length-value fields.

use std::net::Ipv4Addr;

use netlabcraft_core::MacAddr;

use crate::{Result, WireError};

pub const ETHERTYPE_LLDP: u16 = 0x88cc;
/// The nearest-bridge group address: switches take these frames and never forward them.
pub const LLDP_GROUP: MacAddr = MacAddr([0x01, 0x80, 0xc2, 0x00, 0x00, 0x0e]);

/// System capabilities (IEEE 802.1AB table 8-4).
pub const CAP_BRIDGE: u16 = 1 << 2;
pub const CAP_ROUTER: u16 = 1 << 4;
pub const CAP_STATION: u16 = 1 << 7;

/// The fields NetLabCraft sends and reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lldp {
    /// Chassis ID, as a MAC address (subtype 4).
    pub chassis: MacAddr,
    /// Port ID, as the interface name (subtype 5).
    pub port: String,
    /// Seconds the receiver keeps the information.
    pub ttl: u16,
    pub system_name: String,
    pub description: String,
    /// (capabilities, enabled capabilities).
    pub caps: (u16, u16),
    pub address: Option<Ipv4Addr>,
}

fn tlv(out: &mut Vec<u8>, kind: u8, value: &[u8]) {
    let len = value.len().min(511);
    let head = (u16::from(kind) << 9) | u16::try_from(len).unwrap_or(511);
    out.extend_from_slice(&head.to_be_bytes());
    out.extend_from_slice(value.get(..len).unwrap_or_default());
}

impl Lldp {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        tlv(&mut out, 1, &[&[4][..], &self.chassis.0].concat());
        tlv(&mut out, 2, &[&[5][..], self.port.as_bytes()].concat());
        tlv(&mut out, 3, &self.ttl.to_be_bytes());
        tlv(&mut out, 4, self.port.as_bytes());
        tlv(&mut out, 5, self.system_name.as_bytes());
        tlv(&mut out, 6, self.description.as_bytes());
        tlv(&mut out, 7, &[self.caps.0.to_be_bytes(), self.caps.1.to_be_bytes()].concat());
        if let Some(a) = self.address {
            // Address length 5 (subtype + 4 bytes), IPv4 (1), interface numbering unknown (1), no OID.
            tlv(&mut out, 8, &[&[5, 1][..], &a.octets(), &[1, 0, 0, 0, 0, 0]].concat());
        }
        tlv(&mut out, 0, &[]);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Lldp> {
        let mut l = Lldp {
            chassis: MacAddr([0; 6]),
            port: String::new(),
            ttl: 0,
            system_name: String::new(),
            description: String::new(),
            caps: (0, 0),
            address: None,
        };
        let (mut at, mut seen) = (0usize, 0u8);
        loop {
            let head = crate::be16(b, at).ok_or(WireError::Truncated("lldp"))?;
            let (kind, len) = (head >> 9, usize::from(head & 0x1ff));
            let v = b.get(at + 2..at + 2 + len).ok_or(WireError::Truncated("lldp"))?;
            let text = || String::from_utf8_lossy(v).into_owned();
            match kind {
                0 => break,
                1 => {
                    if let (Some(4), Some(m)) = (v.first(), v.get(1..7)) {
                        l.chassis = MacAddr(m.try_into().unwrap_or([0; 6]));
                    }
                    seen |= 1;
                }
                2 => {
                    l.port = String::from_utf8_lossy(v.get(1..).unwrap_or_default()).into_owned();
                    seen |= 2;
                }
                3 => {
                    l.ttl = crate::be16(v, 0).ok_or(WireError::Truncated("lldp"))?;
                    seen |= 4;
                }
                5 => l.system_name = text(),
                6 => l.description = text(),
                7 => l.caps = (crate::be16(v, 0).unwrap_or(0), crate::be16(v, 2).unwrap_or(0)),
                8 if v.first() == Some(&5) && v.get(1) == Some(&1) => {
                    l.address = v.get(2..6).and_then(|o| <[u8; 4]>::try_from(o).ok()).map(Ipv4Addr::from);
                }
                _ => {}
            }
            at += 2 + len;
        }
        if seen != 7 {
            return Err(WireError::Unsupported("lldp", "chassis, port or TTL missing".into()));
        }
        Ok(l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lldp_round_trips_and_survives_truncation() {
        let l = Lldp {
            chassis: MacAddr([2, 0, 0, 0, 0, 1]),
            port: "GigabitEthernet0/1".into(),
            ttl: 120,
            system_name: "R1".into(),
            description: "NetLabCraft Router-2".into(),
            caps: (CAP_ROUTER, CAP_ROUTER),
            address: Some(Ipv4Addr::new(10, 0, 0, 1)),
        };
        let b = l.encode();
        assert_eq!(Lldp::decode(&b).unwrap(), l);
        for k in 0..b.len() {
            let _ = Lldp::decode(&b[..k]);
        }
        assert!(Lldp::decode(&[0, 0]).is_err());
    }
}
