//! LACP (IEEE 802.1AX): the LACPDUs that ports of an EtherChannel exchange to agree on bundling.

use packetcraft_core::MacAddr;

use crate::{Result, WireError, be16};

/// The slow-protocols EtherType and group address.
pub const ETHERTYPE_SLOW: u16 = 0x8809;
pub const SLOW_GROUP: MacAddr = MacAddr([0x01, 0x80, 0xc2, 0x00, 0x00, 0x02]);
const SUBTYPE_LACP: u8 = 1;

/// Port state bits (IEEE 802.1AX §6.4.2.3).
pub const ACTIVITY: u8 = 1;
pub const AGGREGATION: u8 = 1 << 2;
pub const SYNC: u8 = 1 << 3;
pub const COLLECTING: u8 = 1 << 4;
pub const DISTRIBUTING: u8 = 1 << 5;
pub const DEFAULTED: u8 = 1 << 6;

/// What one end says about itself (actor) or what it knows of the other (partner).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LacpInfo {
    pub system_priority: u16,
    pub system: MacAddr,
    pub key: u16,
    pub port_priority: u16,
    pub port: u16,
    pub state: u8,
}

impl LacpInfo {
    fn encode(&self, kind: u8, out: &mut Vec<u8>) {
        out.extend_from_slice(&[kind, 20]);
        out.extend_from_slice(&self.system_priority.to_be_bytes());
        out.extend_from_slice(&self.system.0);
        out.extend_from_slice(&self.key.to_be_bytes());
        out.extend_from_slice(&self.port_priority.to_be_bytes());
        out.extend_from_slice(&self.port.to_be_bytes());
        out.extend_from_slice(&[self.state, 0, 0, 0]);
    }

    fn decode(b: &[u8], at: usize, kind: u8) -> Result<LacpInfo> {
        let t = || WireError::Truncated("lacp");
        let v = b.get(at..at + 20).ok_or_else(t)?;
        if v.first() != Some(&kind) || v.get(1) != Some(&20) {
            return Err(WireError::Unsupported("lacp", format!("TLV {kind} missing")));
        }
        Ok(LacpInfo {
            system_priority: be16(v, 2).ok_or_else(t)?,
            system: MacAddr(v.get(4..10).and_then(|m| m.try_into().ok()).ok_or_else(t)?),
            key: be16(v, 10).ok_or_else(t)?,
            port_priority: be16(v, 12).ok_or_else(t)?,
            port: be16(v, 14).ok_or_else(t)?,
            state: *v.get(16).ok_or_else(t)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lacpdu {
    pub actor: LacpInfo,
    pub partner: LacpInfo,
}

impl Lacpdu {
    /// 110 bytes: header, actor, partner, collector and terminator TLVs, padding.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![SUBTYPE_LACP, 1];
        self.actor.encode(1, &mut out);
        self.partner.encode(2, &mut out);
        out.extend_from_slice(&[3, 16, 0, 0]);
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(&[0, 0]);
        out.resize(110, 0);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Lacpdu> {
        if b.first() != Some(&SUBTYPE_LACP) {
            return Err(WireError::Unsupported("lacp", "not an LACPDU".into()));
        }
        Ok(Lacpdu { actor: LacpInfo::decode(b, 2, 1)?, partner: LacpInfo::decode(b, 22, 2)? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lacpdu_round_trips_and_survives_truncation() {
        let actor = LacpInfo {
            system_priority: 32768,
            system: MacAddr([2, 0, 0, 0, 0, 1]),
            key: 1,
            port_priority: 32768,
            port: 3,
            state: ACTIVITY | AGGREGATION | SYNC,
        };
        let p = Lacpdu { actor, partner: LacpInfo { state: DEFAULTED, ..LacpInfo::default() } };
        let b = p.encode();
        assert_eq!(b.len(), 110);
        assert_eq!(Lacpdu::decode(&b).unwrap(), p);
        for k in 0..b.len() {
            let _ = Lacpdu::decode(&b[..k]);
        }
    }
}
