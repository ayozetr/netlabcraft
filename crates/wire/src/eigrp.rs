//! EIGRP packets (RFC 7868): the fixed header (§6.4) and the hello's TLVs — parameters (§6.6.1)
//! and software version (§6.6.4). Hellos go to `224.0.0.10` as IP protocol 88.

use std::net::Ipv4Addr;

use crate::{Result, WireError, be16, checksum};

/// EIGRP's IP protocol number and its routers group (`224.0.0.10`).
pub const PROTO_EIGRP: u8 = 88;
pub const EIGRP_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 10);
/// The header version this encoder writes.
pub const VERSION: u8 = 2;
pub const OPCODE_UPDATE: u8 = 1;
pub const OPCODE_QUERY: u8 = 3;
pub const OPCODE_REPLY: u8 = 4;
pub const OPCODE_HELLO: u8 = 5;
/// Hello interval and hold time on LAN and point-to-point links (seconds).
pub const HELLO_SECS: u16 = 5;
pub const HOLD_SECS: u16 = 15;
const HEADER_LEN: usize = 20;
const TLV_PARAMETERS: u16 = 0x0001;
const TLV_SOFTWARE: u16 = 0x0004;
/// Most TLVs read from one packet (hostile input).
const MAX_TLVS: usize = 64;

/// The name of an opcode, for the event list.
pub fn opcode_name(op: u8) -> &'static str {
    match op {
        OPCODE_UPDATE => "update",
        OPCODE_QUERY => "query",
        OPCODE_REPLY => "reply",
        OPCODE_HELLO => "hello",
        10 => "SIA query",
        11 => "SIA reply",
        _ => "packet",
    }
}

/// An EIGRP hello: header fields plus the parameter and software-version TLVs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EigrpHello {
    pub asn: u16,
    /// K1..K6: the metric weights; routers only become neighbours when theirs agree.
    pub k: [u8; 6],
    pub hold_secs: u16,
    /// Software version TLV: (OS major, OS minor, EIGRP major, EIGRP minor).
    pub software: Option<[u8; 4]>,
}

impl EigrpHello {
    /// The classic weights: bandwidth and delay only (K1 = K3 = 1).
    pub const DEFAULT_K: [u8; 6] = [1, 0, 1, 0, 0, 0];

    pub fn new(asn: u16) -> EigrpHello {
        EigrpHello { asn, k: Self::DEFAULT_K, hold_secs: HOLD_SECS, software: Some([1, 0, 2, 0]) }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(40);
        v.push(VERSION);
        v.push(OPCODE_HELLO);
        v.extend_from_slice(&[0, 0]); // checksum, filled below
        v.extend_from_slice(&0u32.to_be_bytes()); // flags
        v.extend_from_slice(&0u32.to_be_bytes()); // sequence: hellos are unreliable
        v.extend_from_slice(&0u32.to_be_bytes()); // acknowledgement
        v.extend_from_slice(&0u16.to_be_bytes()); // virtual router id: unicast
        v.extend_from_slice(&self.asn.to_be_bytes());
        v.extend_from_slice(&TLV_PARAMETERS.to_be_bytes());
        v.extend_from_slice(&12u16.to_be_bytes());
        v.extend_from_slice(&self.k);
        v.extend_from_slice(&self.hold_secs.to_be_bytes());
        if let Some(sw) = self.software {
            v.extend_from_slice(&TLV_SOFTWARE.to_be_bytes());
            v.extend_from_slice(&8u16.to_be_bytes());
            v.extend_from_slice(&sw);
        }
        let c = checksum(&[&v]);
        if let Some(s) = v.get_mut(2..4) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        v
    }

    /// The opcode and autonomous system of any EIGRP packet (header only, checksum verified).
    pub fn header(b: &[u8]) -> Result<(u8, u16)> {
        let t = || WireError::Truncated("eigrp");
        if b.len() < HEADER_LEN {
            return Err(t());
        }
        if b.first() != Some(&VERSION) {
            return Err(WireError::Unsupported("eigrp", format!("version {}", b.first().copied().unwrap_or(0))));
        }
        if checksum(&[b]) != 0 {
            return Err(WireError::Checksum("eigrp"));
        }
        Ok((*b.get(1).ok_or_else(t)?, be16(b, 18).ok_or_else(t)?))
    }

    pub fn decode(b: &[u8]) -> Result<EigrpHello> {
        let t = || WireError::Truncated("eigrp");
        let (op, asn) = Self::header(b)?;
        if op != OPCODE_HELLO {
            return Err(WireError::Unsupported("eigrp", format!("not a hello ({})", opcode_name(op))));
        }
        let mut params = None;
        let mut software = None;
        let mut at = HEADER_LEN;
        for _ in 0..MAX_TLVS {
            if at >= b.len() {
                break;
            }
            let kind = be16(b, at).ok_or_else(t)?;
            let len = usize::from(be16(b, at + 2).ok_or_else(t)?);
            if len < 4 {
                return Err(WireError::Unsupported("eigrp", format!("TLV length {len}")));
            }
            let value = b.get(at + 4..at + len).ok_or_else(t)?;
            match kind {
                TLV_PARAMETERS => {
                    let k: [u8; 6] = value.get(..6).ok_or_else(t)?.try_into().map_err(|_| t())?;
                    params = Some((k, be16(value, 6).ok_or_else(t)?));
                }
                TLV_SOFTWARE => {
                    software = Some(value.get(..4).ok_or_else(t)?.try_into().map_err(|_| t())?);
                }
                _ => {}
            }
            at += len;
        }
        let (k, hold_secs) = params.ok_or_else(|| WireError::Unsupported("eigrp", "hello without parameters".into()))?;
        Ok(EigrpHello { asn, k, hold_secs, software })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips_and_survives_truncation() {
        let h = EigrpHello::new(100);
        let v = h.encode();
        assert_eq!(v.len(), 20 + 12 + 8);
        assert_eq!(checksum(&[&v]), 0);
        assert_eq!(EigrpHello::decode(&v).unwrap(), h);
        assert_eq!(EigrpHello::header(&v).unwrap(), (OPCODE_HELLO, 100));
        for n in 0..v.len() {
            assert!(EigrpHello::decode(&v[..n]).is_err(), "{n} bytes");
        }
        let mut bad = v.clone();
        bad[21] ^= 1;
        assert_eq!(EigrpHello::decode(&bad), Err(WireError::Checksum("eigrp")));
        // A hostile TLV length is refused, never trusted.
        let mut odd = EigrpHello { software: None, ..h }.encode();
        odd[23] = 2;
        odd[2] = 0;
        odd[3] = 0;
        let c = checksum(&[&odd]);
        odd[2..4].copy_from_slice(&c.to_be_bytes());
        assert!(EigrpHello::decode(&odd).is_err());
    }
}
