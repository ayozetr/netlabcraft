//! Control-plane messages the simulator puts on the wire so they show in Simulation mode:
//! spanning-tree configuration BPDUs (IEEE 802.1D §9.3.1) and rapid spanning tree BPDUs
//! (802.1D-2004 §9.3.3, the 802.1w format with its role, proposal and agreement flags), OSPFv2 hellos (RFC 2328 A.3.1, A.3.2)
//! and RIPv2 responses (RFC 2453 §4).

use std::net::Ipv4Addr;

use netlabcraft_core::MacAddr;

use crate::{Result, WireError, be16, checksum, ip_at, mac_at};

/// The bridge group address BPDUs go to.
pub const STP_GROUP: MacAddr = MacAddr([0x01, 0x80, 0xc2, 0x00, 0x00, 0x00]);
/// Frames to `01:80:c2:00:00:0x` stay on their link: bridges take them and never forward them
/// (IEEE 802.1Q reserved addresses), and stations without the protocol ignore them.
pub fn link_local(mac: MacAddr) -> bool {
    mac.0[..5] == [0x01, 0x80, 0xc2, 0x00, 0x00] && mac.0[5] < 0x10
}

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

/// RST BPDU flags (IEEE 802.1D-2004 §9.3.3, octet 5).
pub const FLAG_TC: u8 = 0x01;
pub const FLAG_PROPOSAL: u8 = 0x02;
/// The port role sits in bits 3 and 4.
pub const ROLE_SHIFT: u8 = 2;
pub const ROLE_ALTERNATE: u8 = 1;
pub const ROLE_ROOT: u8 = 2;
pub const ROLE_DESIGNATED: u8 = 3;
pub const FLAG_LEARNING: u8 = 0x10;
pub const FLAG_FORWARDING: u8 = 0x20;
pub const FLAG_AGREEMENT: u8 = 0x40;
pub const FLAG_TC_ACK: u8 = 0x80;

/// The words for an RST BPDU's flags.
pub fn rstp_flags_text(flags: u8) -> String {
    let role = match (flags >> ROLE_SHIFT) & 0x03 {
        ROLE_ALTERNATE => "alternate",
        ROLE_ROOT => "root",
        ROLE_DESIGNATED => "designated",
        _ => "unknown",
    };
    let mut words = vec![role];
    for (bit, name) in [
        (FLAG_PROPOSAL, "proposal"),
        (FLAG_AGREEMENT, "agreement"),
        (FLAG_LEARNING, "learning"),
        (FLAG_FORWARDING, "forwarding"),
        (FLAG_TC, "topology change"),
        (FLAG_TC_ACK, "change acknowledged"),
    ] {
        if flags & bit != 0 {
            words.push(name);
        }
    }
    words.join(", ")
}

/// A configuration BPDU (timers in seconds), or with `rstp` an RST BPDU and its flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bpdu {
    pub root: BridgeId,
    pub root_cost: u32,
    pub bridge: BridgeId,
    pub port_id: u16,
    pub message_age: u16,
    /// `Some(flags)`: an RST BPDU (protocol version 2, type 2).
    pub rstp: Option<u8>,
}

impl Bpdu {
    /// The 802.3 payload: LLC header then the BPDU (the frame's type field holds its length).
    pub fn encode(&self) -> Vec<u8> {
        let mut v = LLC_STP.to_vec();
        match self.rstp {
            // Protocol id, version 2, type 2 (RST), flags.
            Some(flags) => v.extend_from_slice(&[0, 0, 2, 2, flags]),
            // Protocol id, version, type (configuration), flags.
            None => v.extend_from_slice(&[0, 0, 0, 0, 0]),
        }
        self.root.encode(&mut v);
        v.extend_from_slice(&self.root_cost.to_be_bytes());
        self.bridge.encode(&mut v);
        v.extend_from_slice(&self.port_id.to_be_bytes());
        for secs in [self.message_age, 20, 2, 15] {
            v.extend_from_slice(&(secs.saturating_mul(256)).to_be_bytes());
        }
        if self.rstp.is_some() {
            v.push(0); // version 1 length
        }
        v
    }

    /// The length field an 802.3 frame carries for a configuration BPDU.
    pub fn frame_length() -> u16 {
        3 + 35
    }

    /// The length field an 802.3 frame carries for this BPDU (an RST BPDU is one byte longer).
    pub fn length(&self) -> u16 {
        if self.rstp.is_some() { 3 + 36 } else { 3 + 35 }
    }

    pub fn decode(b: &[u8]) -> Result<Bpdu> {
        let t = || WireError::Truncated("bpdu");
        if b.get(..3) != Some(&LLC_STP[..]) {
            return Err(WireError::Unsupported("bpdu", "not an LLC 0x42 frame".into()));
        }
        // Protocol id (2 bytes), version, type; then the flags.
        let rstp = match b.get(3..7).ok_or_else(t)? {
            [0, 0, 0, 0] => None,
            [0, 0, 2, 2] => Some(*b.get(7).ok_or_else(t)?),
            _ => return Err(WireError::Unsupported("bpdu", "not a configuration or RST BPDU".into())),
        };
        if rstp.is_some() && b.len() < 3 + 36 {
            return Err(t());
        }
        Ok(Bpdu {
            rstp,
            root: BridgeId::decode(b, 8).ok_or_else(t)?,
            root_cost: u32::from_be_bytes(b.get(16..20).ok_or_else(t)?.try_into().map_err(|_| t())?),
            bridge: BridgeId::decode(b, 20).ok_or_else(t)?,
            port_id: be16(b, 28).ok_or_else(t)?,
            message_age: be16(b, 30).ok_or_else(t)? / 256,
        })
    }
}

/// The authentication field of an OSPF packet (RFC 2328 Appendix D).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OspfAuth {
    /// Type 0.
    Null,
    /// Type 1: the clear-text password, padded with zeros to eight bytes.
    Simple([u8; 8]),
    /// Type 2: the key id, the cryptographic sequence number and the digest after the packet.
    Md5 { key_id: u8, seq: u32, digest: [u8; 16] },
}

/// What to sign an OSPF packet with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OspfAuthSend<'a> {
    Null,
    Simple(&'a str),
    Md5 { key_id: u8, key: &'a str, seq: u32 },
}

/// A key as the digest uses it: at most 16 bytes, padded with zeros (RFC 2328 D.4.3).
fn md5_key(key: &str) -> [u8; 16] {
    let mut k = [0u8; 16];
    for (d, s) in k.iter_mut().zip(key.bytes()) {
        *d = s;
    }
    k
}

fn md5_digest(packet: &[u8], key: &str) -> [u8; 16] {
    let mut msg = packet.to_vec();
    msg.extend_from_slice(&md5_key(key));
    crate::md5::md5(&msg)
}

/// Signs an encoded OSPF packet (one made with no authentication): type 1 writes the password
/// in the packet and keeps the checksum, type 2 zeroes the checksum, fills in the key id, the
/// digest length and the sequence number, and appends the digest of the packet and the key.
pub fn ospf_authenticate(mut packet: Vec<u8>, auth: &OspfAuthSend) -> Vec<u8> {
    if packet.len() < 24 {
        return packet;
    }
    match *auth {
        OspfAuthSend::Null => {}
        OspfAuthSend::Simple(key) => {
            if let Some(f) = packet.get_mut(14..16) {
                f.copy_from_slice(&1u16.to_be_bytes());
            }
            if let Some(f) = packet.get_mut(16..24) {
                for (d, s) in f.iter_mut().zip(key.bytes().chain(std::iter::repeat(0))) {
                    *d = s;
                }
            }
            // The old checksum is zeroed first: the new one is computed over the changed packet.
            if let Some(f) = packet.get_mut(12..14) {
                f.copy_from_slice(&[0, 0]);
            }
            let c = checksum(&[packet.get(..16).unwrap_or_default(), packet.get(24..).unwrap_or_default()]);
            if let Some(f) = packet.get_mut(12..14) {
                f.copy_from_slice(&c.to_be_bytes());
            }
        }
        OspfAuthSend::Md5 { key_id, key, seq } => {
            if let Some(f) = packet.get_mut(12..24) {
                f.copy_from_slice(&[0, 0, 0, 2, 0, 0, key_id, 16, 0, 0, 0, 0]);
            }
            if let Some(f) = packet.get_mut(20..24) {
                f.copy_from_slice(&seq.to_be_bytes());
            }
            let d = md5_digest(&packet, key);
            packet.extend_from_slice(&d);
        }
    }
    packet
}

/// The authentication field of an OSPF packet, `None` for a truncated one or an unknown type.
pub fn ospf_auth_of(packet: &[u8]) -> Option<OspfAuth> {
    match be16(packet, 14)? {
        0 => Some(OspfAuth::Null),
        1 => Some(OspfAuth::Simple(packet.get(16..24)?.try_into().ok()?)),
        2 => {
            let len = usize::from(be16(packet, 2)?);
            Some(OspfAuth::Md5 {
                key_id: *packet.get(18)?,
                seq: u32::from_be_bytes(packet.get(20..24)?.try_into().ok()?),
                digest: packet.get(len..len.checked_add(16)?)?.try_into().ok()?,
            })
        }
        _ => None,
    }
}

/// Whether the digest at the end of a type-2 packet is the digest of the packet under `key`.
pub fn ospf_md5_verify(packet: &[u8], key: &str) -> bool {
    let (Some(OspfAuth::Md5 { digest, .. }), Some(len)) = (ospf_auth_of(packet), be16(packet, 2)) else { return false };
    packet.get(..usize::from(len)).is_some_and(|body| md5_digest(body, key) == digest)
}

/// The checksum of a packet whose authentication is not a digest (type 2 carries none).
fn ospf_checksum_ok(p: &[u8]) -> bool {
    be16(p, 14) == Some(2) || checksum(&[p.get(..16).unwrap_or_default(), p.get(24..).unwrap_or_default()]) == 0
}

/// An OSPFv2 hello (authentication is applied afterwards, by [`ospf_authenticate`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfHello {
    pub router_id: Ipv4Addr,
    pub area: u32,
    pub mask: Ipv4Addr,
    /// HelloInterval and RouterDeadInterval, in seconds: neighbours must agree on both.
    pub hello_secs: u16,
    pub dead_secs: u32,
    pub priority: u8,
    pub dr: Ipv4Addr,
    pub bdr: Ipv4Addr,
    pub neighbors: Vec<Ipv4Addr>,
}

impl OspfHello {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.mask.octets());
        body.extend_from_slice(&self.hello_secs.to_be_bytes());
        body.push(0x02); // options: E bit
        body.push(self.priority);
        body.extend_from_slice(&self.dead_secs.to_be_bytes());
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
        if !ospf_checksum_ok(p) {
            return Err(WireError::Checksum("ospf"));
        }
        let neighbors = (44..len).step_by(4).filter_map(|at| ip_at(p, at)).collect();
        Ok(OspfHello {
            router_id: ip_at(p, 4).ok_or_else(t)?,
            area: u32::from_be_bytes(p.get(8..12).ok_or_else(t)?.try_into().map_err(|_| t())?),
            mask: ip_at(p, 24).ok_or_else(t)?,
            hello_secs: be16(p, 28).ok_or_else(t)?,
            dead_secs: u32::from_be_bytes(p.get(32..36).ok_or_else(t)?.try_into().map_err(|_| t())?),
            priority: *p.get(31).ok_or_else(t)?,
            dr: ip_at(p, 36).ok_or_else(t)?,
            bdr: ip_at(p, 40).ok_or_else(t)?,
            neighbors,
        })
    }
}

/// HSRP's UDP port and its all-routers group (`224.0.0.2`), RFC 2281.
pub const HSRP_PORT: u16 = 1985;
pub const ALL_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 2);

/// The virtual MAC address of an HSRP group (RFC 2281 §5: `00-00-0C-07-AC` + group).
pub fn hsrp_mac(group: u8) -> MacAddr {
    MacAddr([0x00, 0x00, 0x0c, 0x07, 0xac, group])
}

/// An HSRP hello (RFC 2281 §5; hello time 3 s, hold time 10 s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HsrpHello {
    /// `true` from the active router, `false` from the standby.
    pub active: bool,
    pub priority: u8,
    pub group: u8,
    pub virtual_ip: Ipv4Addr,
}

impl HsrpHello {
    pub fn encode(&self) -> Vec<u8> {
        let state = if self.active { 16 } else { 8 };
        let mut v = vec![0, 0, state, 3, 10, self.priority, self.group, 0];
        // The authentication data field: RFC 2281's default text, padded to 8 bytes.
        v.extend_from_slice(b"cisco\0\0\0");
        v.extend_from_slice(&self.virtual_ip.octets());
        v
    }

    pub fn decode(b: &[u8]) -> Result<HsrpHello> {
        let t = || WireError::Truncated("hsrp");
        if b.len() < 20 {
            return Err(t());
        }
        if b[1] != 0 {
            return Err(WireError::Unsupported("hsrp", format!("op code {}", b[1])));
        }
        Ok(HsrpHello { active: b[2] == 16, priority: b[5], group: b[6], virtual_ip: ip_at(b, 16).ok_or_else(t)? })
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

    fn sample_hello() -> OspfHello {
        OspfHello {
            router_id: Ipv4Addr::new(1, 1, 1, 1),
            area: 0,
            mask: Ipv4Addr::new(255, 255, 255, 0),
            hello_secs: 10,
            dead_secs: 40,
            priority: 1,
            dr: Ipv4Addr::UNSPECIFIED,
            bdr: Ipv4Addr::UNSPECIFIED,
            neighbors: vec![Ipv4Addr::new(2, 2, 2, 2)],
        }
    }

    #[test]
    fn ospf_md5_digest_is_computed_over_the_packet_and_the_padded_key() {
        let plain = sample_hello().encode();
        let signed = ospf_authenticate(plain.clone(), &OspfAuthSend::Md5 { key_id: 7, key: "sk-test", seq: 99 });
        // Appendix D.3: type 2, no checksum, key id, digest length 16, sequence number, digest after the packet.
        assert_eq!(signed.len(), plain.len() + 16);
        assert_eq!(&signed[12..24], &[0, 0, 0, 2, 0, 0, 7, 16, 0, 0, 0, 99]);
        // The digest is MD5 of the packet followed by the key padded with zeros to 16 bytes.
        let mut msg = signed[..plain.len()].to_vec();
        msg.extend_from_slice(b"sk-test\0\0\0\0\0\0\0\0\0");
        assert_eq!(&signed[plain.len()..], &crate::md5::md5(&msg));
        assert!(ospf_md5_verify(&signed, "sk-test"));
        assert!(!ospf_md5_verify(&signed, "sk-other"));
        let mut tampered = signed.clone();
        tampered[30] ^= 1;
        assert!(!ospf_md5_verify(&tampered, "sk-test"));
        assert_eq!(ospf_auth_of(&signed), Some(OspfAuth::Md5 { key_id: 7, seq: 99, digest: signed[plain.len()..].try_into().unwrap() }));
        // A hello still decodes, with no checksum to check.
        assert_eq!(OspfHello::decode(&signed).unwrap(), sample_hello());
        for n in 0..signed.len() {
            let _ = OspfHello::decode(&signed[..n]);
            let _ = ospf_auth_of(&signed[..n]);
            let _ = ospf_md5_verify(&signed[..n], "sk-test");
        }
    }

    #[test]
    fn ospf_simple_password_is_in_the_packet_and_the_checksum_holds() {
        let signed = ospf_authenticate(sample_hello().encode(), &OspfAuthSend::Simple("sk-test-long-key"));
        assert_eq!(ospf_auth_of(&signed), Some(OspfAuth::Simple(*b"sk-test-")));
        assert_eq!(OspfHello::decode(&signed).unwrap(), sample_hello());
        let null = ospf_authenticate(sample_hello().encode(), &OspfAuthSend::Null);
        assert_eq!(ospf_auth_of(&null), Some(OspfAuth::Null));
        assert!(!ospf_md5_verify(&null, "x"));
    }

    #[test]
    fn bpdu_round_trips() {
        let b = Bpdu {
            root: BridgeId { priority: 24577, mac: MacAddr([2, 0, 0, 1, 0, 0]) },
            root_cost: 19,
            bridge: BridgeId { priority: 32769, mac: MacAddr([2, 0, 0, 2, 0, 0]) },
            port_id: 0x8001,
            message_age: 1,
            rstp: None,
        };
        let v = b.encode();
        assert_eq!(v.len(), usize::from(Bpdu::frame_length()));
        assert_eq!(Bpdu::decode(&v).unwrap(), b);
        for n in 0..v.len() {
            let _ = Bpdu::decode(&v[..n]);
        }
    }

    #[test]
    fn rst_bpdu_carries_its_flags() {
        let flags = (ROLE_DESIGNATED << ROLE_SHIFT) | FLAG_PROPOSAL | FLAG_LEARNING;
        let b = Bpdu {
            root: BridgeId { priority: 32769, mac: MacAddr([2, 0, 0, 1, 0, 0]) },
            root_cost: 0,
            bridge: BridgeId { priority: 32769, mac: MacAddr([2, 0, 0, 1, 0, 0]) },
            port_id: 0x8002,
            message_age: 0,
            rstp: Some(flags),
        };
        let v = b.encode();
        assert_eq!(v.len(), usize::from(b.length()));
        // LLC, protocol 0000, version 2, type 2, flags 0x1e (designated, proposal, learning).
        assert_eq!(&v[..8], &[0x42, 0x42, 0x03, 0, 0, 2, 2, 0x1e]);
        assert_eq!(v.last(), Some(&0));
        assert_eq!(Bpdu::decode(&v).unwrap(), b);
        assert_eq!(rstp_flags_text(flags), "designated, proposal, learning");
        let agree = (ROLE_ROOT << ROLE_SHIFT) | FLAG_AGREEMENT | FLAG_LEARNING | FLAG_FORWARDING;
        assert_eq!(agree, 0x78);
        assert_eq!(rstp_flags_text(agree), "root, agreement, learning, forwarding");
        for n in 0..v.len() {
            let r = Bpdu::decode(&v[..n]);
            assert!(r.is_err() || n >= 3 + 36, "{n}");
        }
    }

    #[test]
    fn ospf_hello_round_trips_with_its_checksum() {
        let h = OspfHello {
            router_id: Ipv4Addr::new(1, 1, 1, 1),
            area: 0,
            mask: Ipv4Addr::new(255, 255, 255, 252),
            hello_secs: 5,
            dead_secs: 20,
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
