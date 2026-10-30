//! OSPFv3 hellos (RFC 5340 A.3.1, A.3.2): OSPF for IPv6, sent from link-local addresses to
//! `ff02::5`, with the IPv6 pseudo-header in the checksum.

use std::net::{Ipv4Addr, Ipv6Addr};

use crate::{Result, WireError, be16, ip_at};

/// All OSPFv3 routers (`ff02::5`).
pub const ALL_SPF_ROUTERS6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 5);
pub const PROTO_OSPF: u8 = 89;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello6 {
    pub router_id: Ipv4Addr,
    pub area: u32,
    /// The sender's interface ID on this link.
    pub interface_id: u32,
    pub priority: u8,
    pub dr: Ipv4Addr,
    pub bdr: Ipv4Addr,
    pub neighbors: Vec<Ipv4Addr>,
}

fn sum(src: Ipv6Addr, dst: Ipv6Addr, packet: &[u8]) -> u16 {
    let len = u32::try_from(packet.len()).unwrap_or(u32::MAX).to_be_bytes();
    crate::checksum(&[&src.octets(), &dst.octets(), &len, &[0, 0, 0, PROTO_OSPF], packet])
}

impl Hello6 {
    pub fn encode(&self, src: Ipv6Addr, dst: Ipv6Addr) -> Vec<u8> {
        let len = u16::try_from(36 + 4 * self.neighbors.len()).unwrap_or(u16::MAX);
        let mut v = vec![3, 1];
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(&self.router_id.octets());
        v.extend_from_slice(&self.area.to_be_bytes());
        v.extend_from_slice(&[0, 0, 0, 0]); // checksum, instance 0, reserved
        v.extend_from_slice(&self.interface_id.to_be_bytes());
        // Priority, then options: V6, E and R bits.
        v.extend_from_slice(&[self.priority, 0, 0, 0x13]);
        v.extend_from_slice(&10u16.to_be_bytes());
        v.extend_from_slice(&40u16.to_be_bytes());
        v.extend_from_slice(&self.dr.octets());
        v.extend_from_slice(&self.bdr.octets());
        for n in &self.neighbors {
            v.extend_from_slice(&n.octets());
        }
        let c = sum(src, dst, &v);
        if let Some(s) = v.get_mut(12..14) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        v
    }

    pub fn decode(src: Ipv6Addr, dst: Ipv6Addr, b: &[u8]) -> Result<Hello6> {
        let t = || WireError::Truncated("ospfv3");
        if b.first() != Some(&3) || b.get(1) != Some(&1) {
            return Err(WireError::Unsupported("ospfv3", "not an OSPFv3 hello".into()));
        }
        let len = usize::from(be16(b, 2).ok_or_else(t)?);
        let p = b.get(..len).filter(|_| len >= 36).ok_or_else(t)?;
        if sum(src, dst, p) != 0 {
            return Err(WireError::Checksum("ospfv3"));
        }
        Ok(Hello6 {
            router_id: ip_at(p, 4).ok_or_else(t)?,
            area: u32::from_be_bytes(p.get(8..12).ok_or_else(t)?.try_into().map_err(|_| t())?),
            interface_id: u32::from_be_bytes(p.get(16..20).ok_or_else(t)?.try_into().map_err(|_| t())?),
            priority: *p.get(20).ok_or_else(t)?,
            dr: ip_at(p, 28).ok_or_else(t)?,
            bdr: ip_at(p, 32).ok_or_else(t)?,
            neighbors: (36..len).step_by(4).filter_map(|at| ip_at(p, at)).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello6_round_trips_with_its_checksum() {
        let (src, dst): (Ipv6Addr, Ipv6Addr) = ("fe80::1".parse().unwrap(), ALL_SPF_ROUTERS6);
        let h = Hello6 {
            router_id: Ipv4Addr::new(1, 1, 1, 1),
            area: 0,
            interface_id: 3,
            priority: 1,
            dr: Ipv4Addr::new(2, 2, 2, 2),
            bdr: Ipv4Addr::UNSPECIFIED,
            neighbors: vec![Ipv4Addr::new(2, 2, 2, 2)],
        };
        let b = h.encode(src, dst);
        assert_eq!(Hello6::decode(src, dst, &b).unwrap(), h);
        assert!(Hello6::decode("fe80::2".parse().unwrap(), dst, &b).is_err(), "the pseudo-header is checked");
        for k in 0..b.len() {
            let _ = Hello6::decode(src, dst, &b[..k]);
        }
    }
}
