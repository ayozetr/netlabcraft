//! VRRP advertisements: version 2 (RFC 3768 §5) and version 3 for IPv4 (RFC 5798 §5).
//!
//! Both go from the master's interface address to `224.0.0.18` as IP protocol 112 with TTL 255,
//! from the group's virtual MAC address `00-00-5E-00-01-{VRID}`. Version 2 carries the
//! advertisement interval in seconds, an authentication type (always 0, none: RFC 3768 removed
//! the others) and eight bytes of authentication data, and its checksum covers the VRRP message
//! only. Version 3 carries the interval in centiseconds (12 bits), no authentication, and its
//! checksum covers an IPv4 pseudo-header too (§5.2.8).

use std::net::Ipv4Addr;

use netlabcraft_core::MacAddr;

use crate::{Result, WireError, checksum};

/// VRRP's IP protocol number and its IPv4 group.
pub const PROTO_VRRP: u8 = 112;
pub const VRRP_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 18);
/// The type field of an advertisement.
const TYPE_ADVERTISEMENT: u8 = 1;
/// Most addresses one advertisement carries (hostile input).
const MAX_ADDRESSES: usize = 16;

/// The virtual MAC address of an IPv4 virtual router: `00-00-5E-00-01-{VRID}`.
pub fn virtual_mac(vrid: u8) -> MacAddr {
    MacAddr([0x00, 0x00, 0x5e, 0x00, 0x01, vrid])
}

/// A VRRP advertisement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vrrp {
    /// 2 or 3.
    pub version: u8,
    pub vrid: u8,
    /// 255 for the address owner, 0 when the master stops.
    pub priority: u8,
    /// Version 2: seconds; version 3: centiseconds.
    pub interval: u16,
    pub addresses: Vec<Ipv4Addr>,
}

impl Vrrp {
    /// The message. `src` and `dst` are the IP addresses, for version 3's pseudo-header.
    pub fn encode(&self, src: Ipv4Addr, dst: Ipv4Addr) -> Vec<u8> {
        let addrs = self.addresses.get(..self.addresses.len().min(MAX_ADDRESSES)).unwrap_or_default();
        let count = u8::try_from(addrs.len()).unwrap_or(0);
        let mut v = vec![(self.version.min(15) << 4) | TYPE_ADVERTISEMENT, self.vrid, self.priority, count];
        if self.version == 2 {
            // Authentication type 0, the interval in seconds, then the checksum.
            v.extend_from_slice(&[0, u8::try_from(self.interval.min(255)).unwrap_or(1), 0, 0]);
        } else {
            v.extend_from_slice(&(self.interval & 0x0fff).to_be_bytes());
            v.extend_from_slice(&[0, 0]);
        }
        for a in addrs {
            v.extend_from_slice(&a.octets());
        }
        if self.version == 2 {
            v.extend_from_slice(&[0; 8]);
        }
        let c = Self::sum(self.version, src, dst, &v);
        if let Some(s) = v.get_mut(6..8) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        v
    }

    fn sum(version: u8, src: Ipv4Addr, dst: Ipv4Addr, msg: &[u8]) -> u16 {
        if version == 2 {
            return checksum(&[msg]);
        }
        let len = u16::try_from(msg.len()).unwrap_or(u16::MAX).to_be_bytes();
        checksum(&[&src.octets(), &dst.octets(), &[0, PROTO_VRRP], &len, msg])
    }

    pub fn decode(b: &[u8], src: Ipv4Addr, dst: Ipv4Addr) -> Result<Vrrp> {
        let t = || WireError::Truncated("vrrp");
        let first = *b.first().ok_or_else(t)?;
        let version = first >> 4;
        if !(version == 2 || version == 3) {
            return Err(WireError::Unsupported("vrrp", format!("version {version}")));
        }
        if first & 0x0f != TYPE_ADVERTISEMENT {
            return Err(WireError::Unsupported("vrrp", format!("type {}", first & 0x0f)));
        }
        let count = usize::from(*b.get(3).ok_or_else(t)?);
        if count > MAX_ADDRESSES {
            return Err(WireError::Unsupported("vrrp", format!("{count} addresses")));
        }
        let need = 8 + 4 * count + if version == 2 { 8 } else { 0 };
        let msg = b.get(..need).ok_or_else(t)?;
        if Self::sum(version, src, dst, msg) != 0 {
            return Err(WireError::Checksum("vrrp"));
        }
        let interval = if version == 2 {
            u16::from(*msg.get(5).ok_or_else(t)?)
        } else {
            u16::from_be_bytes([*msg.get(4).ok_or_else(t)?, *msg.get(5).ok_or_else(t)?]) & 0x0fff
        };
        let addresses = (0..count)
            .map(|k| {
                let s: [u8; 4] = msg.get(8 + 4 * k..12 + 4 * k).ok_or_else(t)?.try_into().map_err(|_| t())?;
                Ok(Ipv4Addr::from(s))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Vrrp { version, vrid: *msg.get(1).ok_or_else(t)?, priority: *msg.get(2).ok_or_else(t)?, interval, addresses })
    }

    /// One line for the event list.
    pub fn describe(&self) -> String {
        let interval = if self.version == 2 { format!("{} s", self.interval) } else { format!("{} cs", self.interval) };
        let addrs: Vec<String> = self.addresses.iter().map(Ipv4Addr::to_string).collect();
        format!("VRRPv{} advertisement, group {}, priority {}, every {interval}, {}", self.version, self.vrid, self.priority, addrs.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);

    #[test]
    fn version_2_matches_its_layout_and_checksum() {
        let a = Vrrp { version: 2, vrid: 1, priority: 100, interval: 1, addresses: vec![Ipv4Addr::new(192, 168, 1, 254)] };
        let v = a.encode(SRC, VRRP_GROUP);
        // 0x21 (v2, advertisement), VRID 1, priority 100, 1 address, auth 0, interval 1,
        // checksum, the address, 8 bytes of zero authentication data.
        // The checksum by hand: 0x2101 + 0x6401 + 0x0001 + 0xc0a8 + 0x01fe = 0x147a9
        // → 0x47a9 + 1 = 0x47aa, inverted 0xb855.
        assert_eq!(v, [0x21, 1, 100, 1, 0, 1, 0xb8, 0x55, 192, 168, 1, 254, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(Vrrp::decode(&v, SRC, VRRP_GROUP).unwrap(), a);
        for n in 0..v.len() {
            assert!(Vrrp::decode(&v[..n], SRC, VRRP_GROUP).is_err(), "{n}");
        }
        let mut bad = v.clone();
        bad[2] = 101;
        assert_eq!(Vrrp::decode(&bad, SRC, VRRP_GROUP), Err(WireError::Checksum("vrrp")));
        assert_eq!(virtual_mac(1), MacAddr([0, 0, 0x5e, 0, 1, 1]));
    }

    #[test]
    fn version_3_sums_the_pseudo_header() {
        let a = Vrrp { version: 3, vrid: 10, priority: 255, interval: 100, addresses: vec![Ipv4Addr::new(10, 0, 0, 1)] };
        let v = a.encode(Ipv4Addr::new(10, 0, 0, 1), VRRP_GROUP);
        // 0x31, VRID 10, priority 255, 1 address, interval 100 cs (0x0064), checksum, address.
        // By hand: pseudo-header 0x0a00 + 0x0001 + 0xe000 + 0x0012 + 0x0070 + 0x000c = 0xea8f,
        // message 0x310a + 0xff01 + 0x0064 + 0x0a00 + 0x0001 = 0x13a70; together 0x224ff
        // → 0x24ff + 2 = 0x2501, inverted 0xdafe.
        assert_eq!(v, [0x31, 10, 255, 1, 0x00, 0x64, 0xda, 0xfe, 10, 0, 0, 1]);
        assert_eq!(Vrrp::decode(&v, Ipv4Addr::new(10, 0, 0, 1), VRRP_GROUP).unwrap(), a);
        // From another source the pseudo-header differs: the checksum fails.
        assert!(Vrrp::decode(&v, Ipv4Addr::new(10, 0, 0, 2), VRRP_GROUP).is_err());
        for n in 0..v.len() {
            assert!(Vrrp::decode(&v[..n], Ipv4Addr::new(10, 0, 0, 1), VRRP_GROUP).is_err(), "{n}");
        }
        assert!(a.describe().contains("VRRPv3 advertisement, group 10, priority 255, every 100 cs, 10.0.0.1"));
        // Hostile: an address count past the cap, an unknown version.
        assert!(Vrrp::decode(&[0x31, 1, 1, 200, 0, 0, 0, 0], SRC, VRRP_GROUP).is_err());
        assert!(Vrrp::decode(&[0x41, 1, 1, 0, 0, 0, 0, 0], SRC, VRRP_GROUP).is_err());
    }
}
