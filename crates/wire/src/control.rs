//! Control-plane messages the simulator puts on the wire so they show in Simulation mode:
//! spanning-tree configuration BPDUs (IEEE 802.1D §9.3.1), OSPFv2 hellos (RFC 2328 A.3.1, A.3.2)
//! and RIPv2 responses (RFC 2453 §4).

use std::net::Ipv4Addr;

use packetcraft_core::MacAddr;

use crate::{Result, WireError, be16, checksum, ip_at, mac_at};

/// The bridge group address BPDUs go to.
pub const STP_GROUP: MacAddr = MacAddr([0x01, 0x80, 0xc2, 0x00, 0x00, 0x00]);
/// LLC header of a BPDU: DSAP and SSAP 0x42, UI frame.
const LLC_STP: [u8; 3] = [0x42, 0x42, 0x03];

/// OSPF's IP protocol number and its all-routers group (`224.0.0.5`).
pub const PROTO_OSPF: u8 = 89;
pub const OSPF_ALL_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 5);
/// RIP's UDP port and its RIPv2 routers group (`224.0.0.9`).
pub const RIP_PORT: u16 = 520;
pub const RIP_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 9);

/// The Ethernet group address of an IPv4 multicast group (`01:00:5e` + low 23 bits).
pub fn ipv4_multicast_mac(g: Ipv4Addr) -> MacAddr {
    let o = g.octets();
    MacAddr([0x01, 0x00, 0x5e, o[1] & 0x7f, o[2], o[3]])
}

/// A bridge identifier: priority and MAC address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeId {
    pub priority: u16,
    pub mac: MacAddr,
}

impl BridgeId {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.priority.to_be_bytes());
        out.extend_from_slice(&self.mac.0);
    }

    fn decode(b: &[u8], at: usize) -> Option<BridgeId> {
        Some(BridgeId { priority: be16(b, at)?, mac: mac_at(b, at + 2)? })
    }
}

impl std::fmt::Display for BridgeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.priority, self.mac.dotted())
    }
}

/// A configuration BPDU (timers in seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bpdu {
    pub root: BridgeId,
    pub root_cost: u32,
    pub bridge: BridgeId,
    pub port_id: u16,
    pub message_age: u16,
}

impl Bpdu {
    /// The 802.3 payload: LLC header then the BPDU (the frame's type field holds its length).
    pub fn encode(&self) -> Vec<u8> {
        let mut v = LLC_STP.to_vec();
        v.extend_from_slice(&[0, 0, 0, 0, 0]); // protocol id, version, type (configuration), flags
        self.root.encode(&mut v);
        v.extend_from_slice(&self.root_cost.to_be_bytes());
        self.bridge.encode(&mut v);
        v.extend_from_slice(&self.port_id.to_be_bytes());
        for secs in [self.message_age, 20, 2, 15] {
            v.extend_from_slice(&(secs.saturating_mul(256)).to_be_bytes());
        }
        v
    }

    /// The length field an 802.3 frame carries for this BPDU.
    pub fn frame_length() -> u16 {
        3 + 35
    }

    pub fn decode(b: &[u8]) -> Result<Bpdu> {
        let t = || WireError::Truncated("bpdu");
        if b.get(..3) != Some(&LLC_STP[..]) {
            return Err(WireError::Unsupported("bpdu", "not an LLC 0x42 frame".into()));
        }
        if b.get(3..6).ok_or_else(t)? != [0, 0, 0] {
            return Err(WireError::Unsupported("bpdu", "not a configuration BPDU".into()));
        }
        Ok(Bpdu {
            root: BridgeId::decode(b, 8).ok_or_else(t)?,
            root_cost: u32::from_be_bytes(b.get(16..20).ok_or_else(t)?.try_into().map_err(|_| t())?),
            bridge: BridgeId::decode(b, 20).ok_or_else(t)?,
            port_id: be16(b, 28).ok_or_else(t)?,
            message_age: be16(b, 30).ok_or_else(t)? / 256,
        })
    }
}

/// An OSPFv2 hello (area and interval values as used here: hello 10 s, dead 40 s, no auth).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfHello {
    pub router_id: Ipv4Addr,
    pub area: u32,
    pub mask: Ipv4Addr,
    pub priority: u8,
    pub dr: Ipv4Addr,
    pub bdr: Ipv4Addr,
    pub neighbors: Vec<Ipv4Addr>,
}

impl OspfHello {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.mask.octets());
        body.extend_from_slice(&10u16.to_be_bytes());
        body.push(0x02); // options: E bit
        body.push(self.priority);
        body.extend_from_slice(&40u32.to_be_bytes());
        body.extend_from_slice(&self.dr.octets());
        body.extend_from_slice(&self.bdr.octets());
        for n in &self.neighbors {
            body.extend_from_slice(&n.octets());
        }
        let len = u16::try_from(24 + body.len()).unwrap_or(u16::MAX);
        let mut v = vec![2, 1];
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(&self.router_id.octets());
        v.extend_from_slice(&self.area.to_be_bytes());
        v.extend_from_slice(&[0, 0, 0, 0]); // checksum, authentication type 0
        v.extend_from_slice(&[0; 8]); // authentication
        v.extend(body);
        // The checksum covers the packet except the 8 authentication bytes.
        let c = checksum(&[v.get(..16).unwrap_or_default(), v.get(24..).unwrap_or_default()]);
        if let Some(s) = v.get_mut(12..14) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        v
    }

    pub fn decode(b: &[u8]) -> Result<OspfHello> {
        let t = || WireError::Truncated("ospf");
        if b.first() != Some(&2) || b.get(1) != Some(&1) {
            return Err(WireError::Unsupported("ospf", "not an OSPFv2 hello".into()));
        }
        let len = usize::from(be16(b, 2).ok_or_else(t)?);
        let p = b.get(..len).filter(|_| len >= 44).ok_or_else(t)?;
        if checksum(&[p.get(..16).unwrap_or_default(), p.get(24..).unwrap_or_default()]) != 0 {
            return Err(WireError::Checksum("ospf"));
        }
        let neighbors = (44..len).step_by(4).filter_map(|at| ip_at(p, at)).collect();
        Ok(OspfHello {
            router_id: ip_at(p, 4).ok_or_else(t)?,
            area: u32::from_be_bytes(p.get(8..12).ok_or_else(t)?.try_into().map_err(|_| t())?),
            mask: ip_at(p, 24).ok_or_else(t)?,
            priority: *p.get(31).ok_or_else(t)?,
            dr: ip_at(p, 36).ok_or_else(t)?,
            bdr: ip_at(p, 40).ok_or_else(t)?,
            neighbors,
        })
    }
}

/// One route in a RIP response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RipEntry {
    pub net: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub metric: u32,
}

/// Most entries one RIP message carries.
pub const RIP_MAX_ENTRIES: usize = 25;

/// A RIP response (version 1 or 2; version 1 carries no masks).
pub fn rip_encode(version: u8, entries: &[RipEntry]) -> Vec<u8> {
    let mut v = vec![2, version, 0, 0];
    for e in entries.iter().take(RIP_MAX_ENTRIES) {
        v.extend_from_slice(&2u16.to_be_bytes()); // address family: IP
        v.extend_from_slice(&[0, 0]); // route tag
        v.extend_from_slice(&e.net.octets());
        v.extend_from_slice(&(if version >= 2 { e.mask } else { Ipv4Addr::UNSPECIFIED }).octets());
        v.extend_from_slice(&[0, 0, 0, 0]); // next hop: the sender
        v.extend_from_slice(&e.metric.to_be_bytes());
    }
    v
}

/// The version and entries of a RIP response.
pub fn rip_decode(b: &[u8]) -> Result<(u8, Vec<RipEntry>)> {
    let t = || WireError::Truncated("rip");
    if b.first() != Some(&2) {
        return Err(WireError::Unsupported("rip", "not a response".into()));
    }
    let version = *b.get(1).ok_or_else(t)?;
    let entries = b
        .get(4..)
        .unwrap_or_default()
        .as_chunks::<20>()
        .0
        .iter()
        .filter_map(|c| Some(RipEntry { net: ip_at(c, 4)?, mask: ip_at(c, 8)?, metric: u32::from_be_bytes(c.get(16..20)?.try_into().ok()?) }))
        .collect();
    Ok((version, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpdu_round_trips() {
        let b = Bpdu {
            root: BridgeId { priority: 24577, mac: MacAddr([2, 0, 0, 1, 0, 0]) },
            root_cost: 19,
            bridge: BridgeId { priority: 32769, mac: MacAddr([2, 0, 0, 2, 0, 0]) },
            port_id: 0x8001,
            message_age: 1,
        };
        let v = b.encode();
        assert_eq!(v.len(), usize::from(Bpdu::frame_length()));
        assert_eq!(Bpdu::decode(&v).unwrap(), b);
        for n in 0..v.len() {
            let _ = Bpdu::decode(&v[..n]);
        }
    }

    #[test]
    fn ospf_hello_round_trips_with_its_checksum() {
        let h = OspfHello {
            router_id: Ipv4Addr::new(1, 1, 1, 1),
            area: 0,
            mask: Ipv4Addr::new(255, 255, 255, 252),
            priority: 1,
            dr: Ipv4Addr::new(10, 0, 0, 2),
            bdr: Ipv4Addr::new(10, 0, 0, 1),
            neighbors: vec![Ipv4Addr::new(2, 2, 2, 2)],
        };
        let mut v = h.encode();
        assert_eq!(OspfHello::decode(&v).unwrap(), h);
        for n in 0..v.len() {
            let _ = OspfHello::decode(&v[..n]);
        }
        v[30] ^= 1;
        assert_eq!(OspfHello::decode(&v), Err(WireError::Checksum("ospf")));
    }

    #[test]
    fn rip_round_trips() {
        let e = [RipEntry { net: Ipv4Addr::new(10, 0, 0, 0), mask: Ipv4Addr::new(255, 0, 0, 0), metric: 2 }];
        assert_eq!(rip_decode(&rip_encode(2, &e)).unwrap(), (2, e.to_vec()));
        assert_eq!(rip_decode(&rip_encode(1, &e)).unwrap().1[0].mask, Ipv4Addr::UNSPECIFIED);
        assert_eq!(ipv4_multicast_mac(OSPF_ALL_ROUTERS).to_string(), "01:00:5e:00:00:05");
    }
}
