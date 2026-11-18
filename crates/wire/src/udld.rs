//! UDLD probes: what a port says about itself so that the port at the other end of its cable can
//! tell whether the cable works in both directions. A probe goes to a group address that bridges
//! never forward and carries the sender's names and its probe interval.

use netlabcraft_core::MacAddr;

use crate::{Result, WireError};

/// The EtherType of a probe (IEEE 802 local experimental EtherType 1) and its group address
/// (one of the reserved link-local bridge addresses).
pub const ETHERTYPE_UDLD: u16 = 0x88b5;
pub const UDLD_GROUP: MacAddr = MacAddr([0x01, 0x80, 0xc2, 0x00, 0x00, 0x0b]);
const VERSION: u8 = 1;
/// Longest device or port name carried.
pub const MAX_NAME: usize = 40;

/// One probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Seconds between this port's probes.
    pub interval: u8,
    pub device: String,
    pub port: String,
}

impl Probe {
    pub fn encode(&self) -> Vec<u8> {
        let name = |s: &str| -> Vec<u8> {
            let b: Vec<u8> = s.bytes().take(MAX_NAME).collect();
            let mut v = vec![u8::try_from(b.len()).unwrap_or(0)];
            v.extend(b);
            v
        };
        let mut out = vec![VERSION, self.interval];
        out.extend(name(&self.device));
        out.extend(name(&self.port));
        out
    }

    pub fn decode(b: &[u8]) -> Result<Probe> {
        let t = || WireError::Truncated("udld");
        if *b.first().ok_or_else(t)? != VERSION {
            return Err(WireError::Unsupported("udld", "version".into()));
        }
        let interval = *b.get(1).ok_or_else(t)?;
        let name = |at: usize| -> Result<(String, usize)> {
            let n = usize::from(*b.get(at).ok_or_else(t)?);
            let raw = b.get(at + 1..at + 1 + n).ok_or_else(t)?;
            Ok((String::from_utf8_lossy(raw).into_owned(), at + 1 + n))
        };
        let (device, next) = name(2)?;
        let (port, _) = name(next)?;
        Ok(Probe { interval, device, port })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_round_trips_and_every_truncation_is_refused() {
        let p = Probe { interval: 15, device: "S1".into(), port: "GigabitEthernet0/1".into() };
        let b = p.encode();
        assert_eq!(Probe::decode(&b).unwrap(), p);
        for n in 0..b.len() {
            assert!(Probe::decode(&b[..n]).is_err(), "{n} bytes");
        }
    }
}
