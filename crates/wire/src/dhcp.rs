//! DHCP messages (RFC 2131 over the BOOTP layout, options from RFC 2132).
//!
//! Only what an IPv4 lab needs: DISCOVER, OFFER, REQUEST, ACK, NAK, RELEASE with the subnet
//! mask, router, DNS server, lease time, server id and requested address options.

use std::net::Ipv4Addr;

use packetcraft_core::MacAddr;

use crate::{Result, WireError};

pub const SERVER_PORT: u16 = 67;
pub const CLIENT_PORT: u16 = 68;
const MAGIC: [u8; 4] = [99, 130, 83, 99];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpKind {
    Discover,
    Offer,
    Request,
    Ack,
    Nak,
    Release,
}

impl DhcpKind {
    fn code(self) -> u8 {
        match self {
            DhcpKind::Discover => 1,
            DhcpKind::Offer => 2,
            DhcpKind::Request => 3,
            DhcpKind::Ack => 5,
            DhcpKind::Nak => 6,
            DhcpKind::Release => 7,
        }
    }

    fn from_code(c: u8) -> Option<DhcpKind> {
        Some(match c {
            1 => DhcpKind::Discover,
            2 => DhcpKind::Offer,
            3 => DhcpKind::Request,
            5 => DhcpKind::Ack,
            6 => DhcpKind::Nak,
            7 => DhcpKind::Release,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            DhcpKind::Discover => "DISCOVER",
            DhcpKind::Offer => "OFFER",
            DhcpKind::Request => "REQUEST",
            DhcpKind::Ack => "ACK",
            DhcpKind::Nak => "NAK",
            DhcpKind::Release => "RELEASE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dhcp {
    pub kind: DhcpKind,
    pub xid: u32,
    pub client_mac: MacAddr,
    /// `ciaddr`: the client's current address (RELEASE, renewals).
    pub client_ip: Ipv4Addr,
    /// `yiaddr`: the address offered or acknowledged.
    pub your_ip: Ipv4Addr,
    /// `giaddr`: the relay agent, if any.
    pub relay: Ipv4Addr,
    pub mask: Option<Ipv4Addr>,
    pub router: Option<Ipv4Addr>,
    pub dns: Option<Ipv4Addr>,
    pub lease_secs: Option<u32>,
    pub server_id: Option<Ipv4Addr>,
    pub requested: Option<Ipv4Addr>,
    /// Option 150: TFTP servers IP phones fetch their settings from (the first one).
    pub tftp: Option<Ipv4Addr>,
}

impl Dhcp {
    pub fn new(kind: DhcpKind, xid: u32, client_mac: MacAddr) -> Dhcp {
        let z = Ipv4Addr::UNSPECIFIED;
        Dhcp {
            kind,
            xid,
            client_mac,
            client_ip: z,
            your_ip: z,
            relay: z,
            mask: None,
            router: None,
            dns: None,
            lease_secs: None,
            server_id: None,
            requested: None,
            tftp: None,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![0u8; 236];
        b[0] = if matches!(self.kind, DhcpKind::Offer | DhcpKind::Ack | DhcpKind::Nak) { 2 } else { 1 };
        b[1] = 1; // Ethernet
        b[2] = 6;
        b[4..8].copy_from_slice(&self.xid.to_be_bytes());
        b[10] = 0x80; // broadcast flag: answers go to everyone (the client has no address yet)
        b[12..16].copy_from_slice(&self.client_ip.octets());
        b[16..20].copy_from_slice(&self.your_ip.octets());
        b[24..28].copy_from_slice(&self.relay.octets());
        b[28..34].copy_from_slice(&self.client_mac.0);
        b.extend_from_slice(&MAGIC);
        b.extend_from_slice(&[53, 1, self.kind.code()]);
        for (code, v) in [(1u8, self.mask), (3, self.router), (6, self.dns), (54, self.server_id), (50, self.requested), (150, self.tftp)] {
            if let Some(a) = v {
                b.push(code);
                b.push(4);
                b.extend_from_slice(&a.octets());
            }
        }
        if let Some(l) = self.lease_secs {
            b.push(51);
            b.push(4);
            b.extend_from_slice(&l.to_be_bytes());
        }
        b.push(255);
        b
    }

    pub fn decode(b: &[u8]) -> Result<Dhcp> {
        let t = || WireError::Truncated("dhcp");
        if b.len() < 240 {
            return Err(t());
        }
        if b.get(236..240) != Some(&MAGIC[..]) {
            return Err(WireError::Unsupported("dhcp", "no magic cookie".into()));
        }
        let ip = |at: usize| -> Option<Ipv4Addr> { b.get(at..at + 4).map(|s| Ipv4Addr::new(s[0], s[1], s[2], s[3])) };
        let mut mac = [0u8; 6];
        mac.copy_from_slice(b.get(28..34).ok_or_else(t)?);
        let xid = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
        let mut d = Dhcp::new(DhcpKind::Discover, xid, MacAddr(mac));
        d.client_ip = ip(12).ok_or_else(t)?;
        d.your_ip = ip(16).ok_or_else(t)?;
        d.relay = ip(24).ok_or_else(t)?;
        let mut kind = None;
        let mut at = 240;
        while let Some(&code) = b.get(at) {
            match code {
                0 => {
                    at += 1;
                    continue;
                }
                255 => break,
                _ => {}
            }
            let len = usize::from(*b.get(at + 1).ok_or_else(t)?);
            let v = b.get(at + 2..at + 2 + len).ok_or_else(t)?;
            let addr = (len == 4).then(|| Ipv4Addr::new(v[0], v[1], v[2], v[3]));
            match code {
                53 if len == 1 => kind = DhcpKind::from_code(v[0]),
                1 => d.mask = addr,
                3 => d.router = v.get(..4).map(|s| Ipv4Addr::new(s[0], s[1], s[2], s[3])),
                6 => d.dns = v.get(..4).map(|s| Ipv4Addr::new(s[0], s[1], s[2], s[3])),
                54 => d.server_id = addr,
                50 => d.requested = addr,
                150 if len >= 4 => d.tftp = Some(Ipv4Addr::new(v[0], v[1], v[2], v[3])),
                51 if len == 4 => d.lease_secs = Some(u32::from_be_bytes([v[0], v[1], v[2], v[3]])),
                _ => {}
            }
            at += 2 + len;
        }
        d.kind = kind.ok_or_else(|| WireError::Unsupported("dhcp", "no message type".into()))?;
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_hostile_input() {
        let mut d = Dhcp::new(DhcpKind::Ack, 0xdead_beef, MacAddr([2, 0, 0, 0, 0, 9]));
        d.your_ip = Ipv4Addr::new(192, 168, 1, 10);
        d.mask = Some(Ipv4Addr::new(255, 255, 255, 0));
        d.router = Some(Ipv4Addr::new(192, 168, 1, 1));
        d.dns = Some(Ipv4Addr::new(8, 8, 8, 8));
        d.lease_secs = Some(86_400);
        d.server_id = Some(Ipv4Addr::new(192, 168, 1, 1));
        d.tftp = Some(Ipv4Addr::new(192, 168, 100, 1));
        let b = d.encode();
        assert_eq!(Dhcp::decode(&b).unwrap(), d);
        for n in 0..b.len() {
            let _ = Dhcp::decode(&b[..n]);
        }
        let mut bad = b.clone();
        bad[236] = 0;
        assert!(Dhcp::decode(&bad).is_err());
    }
}
