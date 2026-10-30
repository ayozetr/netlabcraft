//! NetLabCraft L1: the bytes that cross a simulated cable.
//!
//! Every frame the simulator moves is real wire format, so a capture opens in any pcap reader.
//! Formats follow the public standards: Ethernet II and IEEE 802.1Q tags, ARP (RFC 826), IPv4
//! (RFC 791), ICMP (RFC 792), UDP (RFC 768), the Internet checksum (RFC 1071), and the classic
//! pcap file format.
//!
//! Decoders treat their input as hostile: lengths are checked, never trusted.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::net::Ipv4Addr;

use netlabcraft_core::MacAddr;

pub mod aes;
pub mod capwap;
pub mod control;
pub mod dhcp;
pub mod dhcp6;
pub mod dns;
pub mod eigrp;
pub mod http;
pub mod ike;
pub mod ipv6;
pub mod lacp;
pub mod lldp;
pub mod md5;

pub mod mgmt;
pub mod netconf;
pub mod ospf6;
pub mod ospfdb;

pub mod pcap;
pub mod qos;
pub mod radius;
pub mod tacacs;

pub mod serial;
pub mod sha1;
pub mod snmp;
pub mod tcp;
pub mod vpn;
pub mod wlan;
pub mod xml;

/// Why bytes didn't decode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("{0}: truncated")]
    Truncated(&'static str),
    #[error("{0}: bad checksum")]
    Checksum(&'static str),
    #[error("{0}: unsupported ({1})")]
    Unsupported(&'static str, String),
}

pub type Result<T> = std::result::Result<T, WireError>;

pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const ETHERTYPE_VLAN: u16 = 0x8100;

pub const PROTO_ICMP: u8 = 1;
pub const PROTO_UDP: u8 = 17;
pub const PROTO_TCP: u8 = 6;
/// FTP's control port, and the ports NetLabCraft's FTP servers open for passive data.
pub const FTP_PORT: u16 = 21;
pub const FTP_DATA_PORTS: std::ops::Range<u16> = 20_000..30_000;
/// SMTP (mail in) and POP3 (mail out to its owner).
pub const SMTP_PORT: u16 = 25;
pub const POP3_PORT: u16 = 110;

/// Shortest Ethernet payload; shorter ones are padded on the wire.
const MIN_PAYLOAD: usize = 46;

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn ip_at(b: &[u8], at: usize) -> Option<Ipv4Addr> {
    let s = b.get(at..at + 4)?;
    Some(Ipv4Addr::new(s[0], s[1], s[2], s[3]))
}

fn mac_at(b: &[u8], at: usize) -> Option<MacAddr> {
    let s = b.get(at..at + 6)?;
    let mut m = [0u8; 6];
    m.copy_from_slice(s);
    Some(MacAddr(m))
}

/// The Internet checksum (RFC 1071): one's-complement sum of 16-bit words.
pub fn checksum(chunks: &[&[u8]]) -> u16 {
    let mut sum: u32 = 0;
    let mut odd: Option<u8> = None;
    for chunk in chunks {
        for &byte in *chunk {
            match odd.take() {
                Some(hi) => sum = sum.wrapping_add(u32::from(u16::from_be_bytes([hi, byte]))),
                None => odd = Some(byte),
            }
        }
    }
    if let Some(hi) = odd {
        sum = sum.wrapping_add(u32::from(u16::from_be_bytes([hi, 0])));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// An Ethernet II frame, optionally 802.1Q tagged. The FCS is not modelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub dst: MacAddr,
    pub src: MacAddr,
    /// The 802.1Q VLAN id, when tagged (priority bits are always 0).
    pub vlan: Option<u16>,
    pub ethertype: u16,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(18 + self.payload.len().max(MIN_PAYLOAD));
        out.extend_from_slice(&self.dst.0);
        out.extend_from_slice(&self.src.0);
        if let Some(v) = self.vlan {
            out.extend_from_slice(&ETHERTYPE_VLAN.to_be_bytes());
            out.extend_from_slice(&(v & 0x0fff).to_be_bytes());
        }
        out.extend_from_slice(&self.ethertype.to_be_bytes());
        out.extend_from_slice(&self.payload);
        let header = if self.vlan.is_some() { 18 } else { 14 };
        if out.len() < header + MIN_PAYLOAD {
            out.resize(header + MIN_PAYLOAD, 0);
        }
        out
    }

    pub fn decode(b: &[u8]) -> Result<Frame> {
        let t = || WireError::Truncated("ethernet");
        let dst = mac_at(b, 0).ok_or_else(t)?;
        let src = mac_at(b, 6).ok_or_else(t)?;
        let mut ethertype = be16(b, 12).ok_or_else(t)?;
        let mut at = 14;
        let mut vlan = None;
        if ethertype == ETHERTYPE_VLAN {
            vlan = Some(be16(b, 14).ok_or_else(t)? & 0x0fff);
            ethertype = be16(b, 16).ok_or_else(t)?;
            at = 18;
        }
        let payload = b.get(at..).ok_or_else(t)?.to_vec();
        Ok(Frame { dst, src, vlan, ethertype, payload })
    }

    /// The same frame with its tag replaced (`None` strips it).
    pub fn retagged(&self, vlan: Option<u16>) -> Frame {
        Frame { vlan, ..self.clone() }
    }
}

/// ARP operation codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpOp {
    Request,
    Reply,
}

/// An ARP packet for IPv4 over Ethernet (RFC 826).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arp {
    pub op: ArpOp,
    pub sender_mac: MacAddr,
    pub sender_ip: Ipv4Addr,
    pub target_mac: MacAddr,
    pub target_ip: Ipv4Addr,
}

impl Arp {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(28);
        out.extend_from_slice(&1u16.to_be_bytes()); // hardware: Ethernet
        out.extend_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
        out.push(6);
        out.push(4);
        out.extend_from_slice(&(if self.op == ArpOp::Request { 1u16 } else { 2u16 }).to_be_bytes());
        out.extend_from_slice(&self.sender_mac.0);
        out.extend_from_slice(&self.sender_ip.octets());
        out.extend_from_slice(&self.target_mac.0);
        out.extend_from_slice(&self.target_ip.octets());
        out
    }

    pub fn decode(b: &[u8]) -> Result<Arp> {
        let t = || WireError::Truncated("arp");
        if be16(b, 0).ok_or_else(t)? != 1 || be16(b, 2).ok_or_else(t)? != ETHERTYPE_IPV4 || b.get(4) != Some(&6) || b.get(5) != Some(&4) {
            return Err(WireError::Unsupported("arp", "not IPv4 over Ethernet".into()));
        }
        let op = match be16(b, 6).ok_or_else(t)? {
            1 => ArpOp::Request,
            2 => ArpOp::Reply,
            other => return Err(WireError::Unsupported("arp", format!("operation {other}"))),
        };
        Ok(Arp {
            op,
            sender_mac: mac_at(b, 8).ok_or_else(t)?,
            sender_ip: ip_at(b, 14).ok_or_else(t)?,
            target_mac: mac_at(b, 18).ok_or_else(t)?,
            target_ip: ip_at(b, 24).ok_or_else(t)?,
        })
    }
}

/// An IPv4 datagram (RFC 791). Options are skipped on decode and never written; fragments are
/// not modelled (the DF bit is always set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ipv4 {
    pub id: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub payload: Vec<u8>,
}

impl Ipv4 {
    pub fn encode(&self) -> Vec<u8> {
        let total = u16::try_from(20 + self.payload.len()).unwrap_or(u16::MAX);
        let mut h = [0u8; 20];
        h[0] = 0x45;
        h[2..4].copy_from_slice(&total.to_be_bytes());
        h[4..6].copy_from_slice(&self.id.to_be_bytes());
        h[6] = 0x40; // DF
        h[8] = self.ttl;
        h[9] = self.protocol;
        h[12..16].copy_from_slice(&self.src.octets());
        h[16..20].copy_from_slice(&self.dst.octets());
        let c = checksum(&[&h]);
        h[10..12].copy_from_slice(&c.to_be_bytes());
        let mut out = h.to_vec();
        let keep = usize::from(total).saturating_sub(20);
        out.extend_from_slice(self.payload.get(..keep).unwrap_or(&self.payload));
        out
    }

    pub fn decode(b: &[u8]) -> Result<Ipv4> {
        let t = || WireError::Truncated("ipv4");
        let vihl = *b.first().ok_or_else(t)?;
        if vihl >> 4 != 4 {
            return Err(WireError::Unsupported("ipv4", format!("version {}", vihl >> 4)));
        }
        let ihl = usize::from(vihl & 0x0f) * 4;
        if ihl < 20 {
            return Err(WireError::Unsupported("ipv4", format!("header length {ihl}")));
        }
        let total = usize::from(be16(b, 2).ok_or_else(t)?);
        if total < ihl || b.len() < total {
            return Err(t());
        }
        let header = b.get(..ihl).ok_or_else(t)?;
        if checksum(&[header]) != 0 {
            return Err(WireError::Checksum("ipv4"));
        }
        Ok(Ipv4 {
            id: be16(b, 4).ok_or_else(t)?,
            ttl: *b.get(8).ok_or_else(t)?,
            protocol: *b.get(9).ok_or_else(t)?,
            src: ip_at(b, 12).ok_or_else(t)?,
            dst: ip_at(b, 16).ok_or_else(t)?,
            payload: b.get(ihl..total).ok_or_else(t)?.to_vec(),
        })
    }
}

/// The ICMP messages the simulator speaks (RFC 792).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icmp {
    EchoRequest {
        id: u16,
        seq: u16,
        data: Vec<u8>,
    },
    EchoReply {
        id: u16,
        seq: u16,
        data: Vec<u8>,
    },
    /// Type 3. `original` is the offending datagram's header plus its first 8 payload bytes.
    Unreachable {
        code: u8,
        original: Vec<u8>,
    },
    /// Type 11, code 0 (TTL exceeded in transit).
    TimeExceeded {
        original: Vec<u8>,
    },
}

/// Destination-unreachable codes used here.
pub const UNREACH_NET: u8 = 0;
pub const UNREACH_HOST: u8 = 1;
pub const UNREACH_PORT: u8 = 3;
/// Communication administratively prohibited (an access list said no).
pub const UNREACH_ADMIN: u8 = 13;

impl Icmp {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = match self {
            Icmp::EchoRequest { id, seq, data } | Icmp::EchoReply { id, seq, data } => {
                let ty = if matches!(self, Icmp::EchoRequest { .. }) { 8 } else { 0 };
                let mut v = vec![ty, 0, 0, 0];
                v.extend_from_slice(&id.to_be_bytes());
                v.extend_from_slice(&seq.to_be_bytes());
                v.extend_from_slice(data);
                v
            }
            Icmp::Unreachable { code, original } => {
                let mut v = vec![3, *code, 0, 0, 0, 0, 0, 0];
                v.extend_from_slice(original);
                v
            }
            Icmp::TimeExceeded { original } => {
                let mut v = vec![11, 0, 0, 0, 0, 0, 0, 0];
                v.extend_from_slice(original);
                v
            }
        };
        let c = checksum(&[&out]);
        if let Some(s) = out.get_mut(2..4) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        out
    }

    pub fn decode(b: &[u8]) -> Result<Icmp> {
        let t = || WireError::Truncated("icmp");
        if b.len() < 8 {
            return Err(t());
        }
        if checksum(&[b]) != 0 {
            return Err(WireError::Checksum("icmp"));
        }
        let (ty, code) = (b[0], b[1]);
        let rest = b.get(8..).unwrap_or_default().to_vec();
        match ty {
            0 | 8 => {
                let id = be16(b, 4).ok_or_else(t)?;
                let seq = be16(b, 6).ok_or_else(t)?;
                Ok(if ty == 8 { Icmp::EchoRequest { id, seq, data: rest } } else { Icmp::EchoReply { id, seq, data: rest } })
            }
            3 => Ok(Icmp::Unreachable { code, original: rest }),
            11 => Ok(Icmp::TimeExceeded { original: rest }),
            other => Err(WireError::Unsupported("icmp", format!("type {other}"))),
        }
    }

    /// The bytes an error message quotes: the datagram's header and first 8 payload bytes.
    pub fn quote(datagram: &[u8]) -> Vec<u8> {
        let ihl = datagram.first().map_or(20, |v| usize::from(v & 0x0f) * 4);
        datagram.get(..(ihl + 8).min(datagram.len())).unwrap_or_default().to_vec()
    }

    /// For an error message about one of our echo requests: its (id, seq).
    pub fn quoted_echo(original: &[u8]) -> Option<(u16, u16)> {
        let ihl = usize::from(original.first()? & 0x0f) * 4;
        if *original.get(9)? != PROTO_ICMP || *original.get(ihl)? != 8 {
            return None;
        }
        Some((be16(original, ihl + 4)?, be16(original, ihl + 6)?))
    }
}

/// A UDP datagram (RFC 768).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Udp {
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: Vec<u8>,
}

impl Udp {
    /// Encodes with the checksum over the IPv4 pseudo-header.
    pub fn encode(&self, src: Ipv4Addr, dst: Ipv4Addr) -> Vec<u8> {
        let len = u16::try_from(8 + self.payload.len()).unwrap_or(u16::MAX);
        let mut out = Vec::with_capacity(usize::from(len));
        out.extend_from_slice(&self.src_port.to_be_bytes());
        out.extend_from_slice(&self.dst_port.to_be_bytes());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&self.payload);
        let pseudo = pseudo_header(src, dst, PROTO_UDP, len);
        let mut c = checksum(&[&pseudo, &out]);
        if c == 0 {
            c = 0xffff;
        }
        if let Some(s) = out.get_mut(6..8) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        out
    }

    pub fn decode(b: &[u8], src: Ipv4Addr, dst: Ipv4Addr) -> Result<Udp> {
        let t = || WireError::Truncated("udp");
        let len = be16(b, 4).ok_or_else(t)?;
        let body = b.get(..usize::from(len)).ok_or_else(t)?;
        if len < 8 {
            return Err(t());
        }
        if be16(b, 6) != Some(0) && checksum(&[&pseudo_header(src, dst, PROTO_UDP, len), body]) != 0 {
            return Err(WireError::Checksum("udp"));
        }
        Ok(Udp { src_port: be16(b, 0).ok_or_else(t)?, dst_port: be16(b, 2).ok_or_else(t)?, payload: body.get(8..).unwrap_or_default().to_vec() })
    }
}

pub(crate) fn pseudo_header(src: Ipv4Addr, dst: Ipv4Addr, proto: u8, len: u16) -> [u8; 12] {
    let mut p = [0u8; 12];
    p[0..4].copy_from_slice(&src.octets());
    p[4..8].copy_from_slice(&dst.octets());
    p[9] = proto;
    p[10..12].copy_from_slice(&len.to_be_bytes());
    p
}

/// The protocol family of a frame, for colouring and filtering events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protocol {
    Arp,
    Icmp,
    Dhcp,
    Dns,
    Http,
    Tcp,
    Udp,
    Ipv4,
    /// Spanning-tree BPDUs.
    Stp,
    Ospf,
    Rip,
    Eigrp,
    Hsrp,
    Radius,
    Ftp,
    Smtp,
    Pop3,
    Tacacs,
    Snmp,
    Lacp,
    Lldp,
    Ntp,
    Syslog,
    Tftp,
    Icmpv6,
    /// IPv6 neighbour discovery (neighbour and router solicitations and advertisements).
    Ndp,
    Ipv6,
    /// Generic Routing Encapsulation (tunnels).
    Gre,
    /// IPsec Encapsulating Security Payload.
    Esp,
    /// ISAKMP / IKEv1 key exchange (UDP 500).
    Isakmp,
    /// IEEE 802.11 management frames over the air (probe, authentication, association).
    Wlan,
    /// IEEE 802.1X EAPOL: EAP logins and the WPA2 4-way handshake.
    Eapol,
    /// CAPWAP between lightweight access points and their controller.
    Capwap,
    Other,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Arp => "ARP",
            Protocol::Icmp => "ICMP",
            Protocol::Dhcp => "DHCP",
            Protocol::Dns => "DNS",
            Protocol::Http => "HTTP",
            Protocol::Tcp => "TCP",
            Protocol::Udp => "UDP",
            Protocol::Ipv4 => "IPv4",
            Protocol::Stp => "STP",
            Protocol::Ospf => "OSPF",
            Protocol::Rip => "RIP",
            Protocol::Eigrp => "EIGRP",
            Protocol::Hsrp => "HSRP",
            Protocol::Lacp => "LACP",
            Protocol::Radius => "RADIUS",
            Protocol::Ftp => "FTP",
            Protocol::Smtp => "SMTP",
            Protocol::Pop3 => "POP3",
            Protocol::Tacacs => "TACACS+",
            Protocol::Snmp => "SNMP",
            Protocol::Lldp => "LLDP",
            Protocol::Ntp => "NTP",
            Protocol::Syslog => "Syslog",
            Protocol::Tftp => "TFTP",
            Protocol::Icmpv6 => "ICMPv6",
            Protocol::Ndp => "NDP",
            Protocol::Ipv6 => "IPv6",
            Protocol::Gre => "GRE",
            Protocol::Esp => "ESP",
            Protocol::Isakmp => "ISAKMP",
            Protocol::Wlan => "802.11",
            Protocol::Eapol => "EAPOL",
            Protocol::Capwap => "CAPWAP",
            Protocol::Other => "Other",
        }
    }
}

/// A one-line, human description of a frame plus its protocol family, as the event list shows.
pub fn summarize(bytes: &[u8]) -> (Protocol, String) {
    summarize_depth(bytes, 0)
}

/// [`summarize`], `depth` encapsulations deep (a tunnel inside a tunnel is described a few
/// levels down, never without end).
fn summarize_depth(bytes: &[u8], depth: u8) -> (Protocol, String) {
    let Ok(f) = Frame::decode(bytes) else { return (Protocol::Other, "malformed frame".into()) };
    let tag = f.vlan.map(|v| format!(" [VLAN {v}]")).unwrap_or_default();
    match f.ethertype {
        ETHERTYPE_ARP => match Arp::decode(&f.payload) {
            Ok(a) if a.op == ArpOp::Request => (Protocol::Arp, format!("ARP who has {}? tell {}{tag}", a.target_ip, a.sender_ip)),
            Ok(a) => (Protocol::Arp, format!("ARP {} is at {}{tag}", a.sender_ip, a.sender_mac)),
            Err(e) => (Protocol::Arp, format!("ARP ({e}){tag}")),
        },
        ETHERTYPE_IPV4 => match Ipv4::decode(&f.payload) {
            Ok(ip) => {
                let route = format!("{} → {}", ip.src, ip.dst);
                match ip.protocol {
                    PROTO_ICMP => {
                        let what = match Icmp::decode(&ip.payload) {
                            Ok(Icmp::EchoRequest { seq, .. }) => format!("echo request seq {seq}"),
                            Ok(Icmp::EchoReply { seq, .. }) => format!("echo reply seq {seq}"),
                            Ok(Icmp::Unreachable { code, .. }) => format!("destination unreachable (code {code})"),
                            Ok(Icmp::TimeExceeded { .. }) => "time exceeded".to_string(),
                            Err(e) => e.to_string(),
                        };
                        (Protocol::Icmp, format!("ICMP {what} {route} ttl {}{tag}", ip.ttl))
                    }
                    control::PROTO_OSPF => match control::OspfHello::decode(&ip.payload) {
                        Ok(h) => (
                            Protocol::Ospf,
                            format!(
                                "OSPF hello from router {} ({} neighbour{}) {route}{tag}",
                                h.router_id,
                                h.neighbors.len(),
                                if h.neighbors.len() == 1 { "" } else { "s" }
                            ),
                        ),
                        Err(_) if ip.payload.get(1).is_some_and(|k| (2..=5).contains(k)) => match ospfdb::OspfDb::decode(&ip.payload) {
                            Ok((r, db)) => {
                                let n = |k: usize, what: &str| format!("{k} {what}{}", if k == 1 { "" } else { "s" });
                                let text = match db {
                                    ospfdb::OspfDb::Dbd { flags, .. } if flags & ospfdb::DBD_INIT != 0 => {
                                        format!("database description, first{}", if flags & ospfdb::DBD_MASTER != 0 { " (master)" } else { "" })
                                    }
                                    ospfdb::OspfDb::Dbd { headers, .. } => format!("database description, {}", n(headers.len(), "LSA header")),
                                    ospfdb::OspfDb::Request(l) => format!("link-state request, {}", n(l.len(), "LSA")),
                                    ospfdb::OspfDb::Update(l) => format!("link-state update, {}", n(l.len(), "LSA")),
                                    ospfdb::OspfDb::Ack(l) => format!("link-state acknowledgement, {}", n(l.len(), "LSA")),
                                };
                                (Protocol::Ospf, format!("OSPF {text} from router {r} {route}{tag}"))
                            }
                            Err(e) => (Protocol::Ospf, format!("OSPF ({e}) {route}{tag}")),
                        },
                        Err(e) => (Protocol::Ospf, format!("OSPF ({e}) {route}{tag}")),
                    },
                    eigrp::PROTO_EIGRP => match eigrp::EigrpHello::header(&ip.payload) {
                        Ok((eigrp::OPCODE_HELLO, _)) => match eigrp::EigrpHello::decode(&ip.payload) {
                            Ok(h) => (Protocol::Eigrp, format!("EIGRP hello, AS {} (hold {} s) {route}{tag}", h.asn, h.hold_secs)),
                            Err(_) => match eigrp::EigrpPacket::decode(&ip.payload) {
                                Ok(p) if p.ack != 0 && p.tlvs.is_empty() => {
                                    (Protocol::Eigrp, format!("EIGRP acknowledgement of {}, AS {} {route}{tag}", p.ack, p.asn))
                                }
                                Ok(p) => (Protocol::Eigrp, format!("EIGRP hello, AS {} {route}{tag}", p.asn)),
                                Err(e) => (Protocol::Eigrp, format!("EIGRP ({e}) {route}{tag}")),
                            },
                        },
                        Ok((op, asn)) => match eigrp::EigrpPacket::decode(&ip.payload) {
                            Ok(p) => {
                                let n = p.routes().count();
                                let down = p.routes().filter(|r| r.is_unreachable()).count();
                                let first = p.routes().next().map(|r| format!(": {}{}", r.net, if n > 1 { ", …" } else { "" })).unwrap_or_default();
                                let unreach = if down > 0 { format!(", {down} unreachable") } else { String::new() };
                                let init = if p.flags & eigrp::FLAG_INIT != 0 { " (init)" } else { "" };
                                (
                                    Protocol::Eigrp,
                                    format!(
                                        "EIGRP {}{init} #{}, AS {asn}, {n} route{}{unreach}{first} {route}{tag}",
                                        eigrp::opcode_name(op),
                                        p.seq,
                                        if n == 1 { "" } else { "s" }
                                    ),
                                )
                            }
                            Err(e) => (Protocol::Eigrp, format!("EIGRP {} ({e}) {route}{tag}", eigrp::opcode_name(op))),
                        },
                        Err(e) => (Protocol::Eigrp, format!("EIGRP ({e}) {route}{tag}")),
                    },
                    PROTO_UDP => match Udp::decode(&ip.payload, ip.src, ip.dst) {
                        Ok(u) if u.dst_port == ike::ISAKMP_PORT && u.src_port == ike::ISAKMP_PORT => match ike::Isakmp::decode(&u.payload) {
                            Ok(m) => {
                                let exchange = match m.exchange {
                                    ike::EXCH_MAIN => "main mode",
                                    ike::EXCH_QUICK => "quick mode",
                                    ike::EXCH_INFO => "informational",
                                    _ => "exchange",
                                };
                                let what = if m.flags & ike::FLAG_ENCRYPTED != 0 {
                                    "(encrypted)".to_string()
                                } else {
                                    let names: Vec<String> = m
                                        .payloads
                                        .iter()
                                        .map(|p| match p {
                                            ike::Payload::Notify { kind, .. } => format!("NOTIFY {}", ike::notify_name(*kind)),
                                            ike::Payload::Sa(list) => {
                                                let n = list.iter().map(|x| x.transforms.len()).sum::<usize>();
                                                format!("SA, {n} transform{}", if n == 1 { "" } else { "s" })
                                            }
                                            p => p.name().to_string(),
                                        })
                                        .collect();
                                    format!("[{}]", names.join(", "))
                                };
                                (Protocol::Isakmp, format!("ISAKMP {exchange} {what} {route}{tag}"))
                            }
                            Err(e) => (Protocol::Isakmp, format!("ISAKMP ({e}) {route}{tag}")),
                        },
                        Ok(u) if [u.dst_port, u.src_port].contains(&snmp::SNMP_PORT) && snmp::version(&u.payload) == Some(3) => {
                            match snmp::V3::decode(&u.payload) {
                                Ok((m, _)) => (
                                    Protocol::Snmp,
                                    format!(
                                        "SNMPv3 {} (user {}{}) {}{tag}",
                                        m.pdu.kind.name(),
                                        if m.user.is_empty() { "-" } else { m.user.as_str() },
                                        if m.flags & snmp::FLAG_AUTH != 0 { ", authenticated" } else { "" },
                                        m.pdu.bindings.first().map(|(o, _)| snmp::oid_text(o)).unwrap_or_default(),
                                    ),
                                ),
                                Err(e) => (Protocol::Snmp, format!("SNMPv3 ({e}){tag}")),
                            }
                        }
                        Ok(u) if u.dst_port == snmp::SNMP_TRAP_PORT => match snmp::Snmp::decode(&u.payload) {
                            Ok(m) => {
                                let what = m.bindings.iter().find_map(|(o, v)| match v {
                                    snmp::Value::Oid(t) if o.as_slice() == snmp::oids::SNMP_TRAP_OID => {
                                        Some(snmp::oids::trap_name(t).map_or_else(|| snmp::oid_text(t), str::to_string))
                                    }
                                    _ => None,
                                });
                                (Protocol::Snmp, format!("SNMP {} ({}) {}{tag}", m.kind.name(), m.community, what.unwrap_or_default()))
                            }
                            Err(e) => (Protocol::Snmp, format!("SNMP ({e}){tag}")),
                        },
                        Ok(u) if u.dst_port == snmp::SNMP_PORT || u.src_port == snmp::SNMP_PORT => match snmp::Snmp::decode(&u.payload) {
                            Ok(m) => (
                                Protocol::Snmp,
                                format!(
                                    "SNMP {} ({}) {}{}{tag}",
                                    m.kind.name(),
                                    m.community,
                                    m.bindings.first().map(|(o, _)| snmp::oid_text(o)).unwrap_or_default(),
                                    m.bindings
                                        .first()
                                        .filter(|_| m.kind == snmp::PduKind::Response)
                                        .map(|(_, v)| format!(" = {}", v.show()))
                                        .unwrap_or_default()
                                ),
                            ),
                            Err(e) => (Protocol::Snmp, format!("SNMP ({e}){tag}")),
                        },
                        Ok(u) if u.dst_port == radius::RADIUS_PORT || u.src_port == radius::RADIUS_PORT => match radius::Radius::decode(&u.payload) {
                            Ok(r) => (
                                Protocol::Radius,
                                format!(
                                    "RADIUS {} {} → {}{}{tag}",
                                    r.name(),
                                    ip.src,
                                    ip.dst,
                                    r.user().map(|u| format!(" (user {u})")).unwrap_or_default()
                                ),
                            ),
                            Err(e) => (Protocol::Radius, format!("RADIUS ({e}){tag}")),
                        },
                        Ok(u) if u.dst_port == mgmt::NTP_PORT || u.src_port == mgmt::NTP_PORT => match mgmt::Ntp::decode(&u.payload) {
                            Ok(n) if n.server => (Protocol::Ntp, format!("NTP time from {} (stratum {}){tag}", ip.src, n.stratum)),
                            Ok(_) => (Protocol::Ntp, format!("NTP time request {} → {}{tag}", ip.src, ip.dst)),
                            Err(e) => (Protocol::Ntp, format!("NTP ({e}){tag}")),
                        },
                        Ok(u) if u.dst_port == mgmt::SYSLOG_PORT => match mgmt::syslog_decode(&u.payload) {
                            Ok((host, msg)) => {
                                (Protocol::Syslog, format!("Syslog from {host} ({}): {}{tag}", ip.src, msg.chars().take(60).collect::<String>()))
                            }
                            Err(e) => (Protocol::Syslog, format!("Syslog ({e}){tag}")),
                        },
                        Ok(u) if u.dst_port == mgmt::TFTP_PORT || u.src_port == mgmt::TFTP_PORT || (u.src_port >= 49_152 && u.dst_port >= 49_152) => {
                            match mgmt::Tftp::decode(&u.payload) {
                                Ok(mgmt::Tftp::Read(n)) => (Protocol::Tftp, format!("TFTP read request for {n} {route}{tag}")),
                                Ok(mgmt::Tftp::Write(n)) => (Protocol::Tftp, format!("TFTP write request for {n} {route}{tag}")),
                                Ok(mgmt::Tftp::Data(b, d)) => (Protocol::Tftp, format!("TFTP data block {b} ({} bytes) {route}{tag}", d.len())),
                                Ok(mgmt::Tftp::Ack(b)) => (Protocol::Tftp, format!("TFTP ack {b} {route}{tag}")),
                                Ok(mgmt::Tftp::Error(_, m)) => (Protocol::Tftp, format!("TFTP error: {m} {route}{tag}")),
                                Err(_) => (Protocol::Udp, format!("UDP {}:{} → {}:{}{tag}", ip.src, u.src_port, ip.dst, u.dst_port)),
                            }
                        }
                        Ok(u) if u.dst_port == control::HSRP_PORT => match control::HsrpHello::decode(&u.payload) {
                            Ok(h) => (
                                Protocol::Hsrp,
                                format!(
                                    "HSRP hello, group {}: {} {} (priority {}), virtual address {}{tag}",
                                    h.group,
                                    ip.src,
                                    if h.active { "active" } else { "standby" },
                                    h.priority,
                                    h.virtual_ip
                                ),
                            ),
                            Err(e) => (Protocol::Hsrp, format!("HSRP ({e}){tag}")),
                        },
                        Ok(u) if u.dst_port == control::RIP_PORT => match control::rip_decode(&u.payload) {
                            Ok((v, entries)) => (
                                Protocol::Rip,
                                format!("RIPv{v} update from {}: {} route{}{tag}", ip.src, entries.len(), if entries.len() == 1 { "" } else { "s" }),
                            ),
                            Err(e) => (Protocol::Rip, format!("RIP ({e}){tag}")),
                        },
                        Ok(u) if u.dst_port == dhcp::SERVER_PORT || u.dst_port == dhcp::CLIENT_PORT => match dhcp::Dhcp::decode(&u.payload) {
                            Ok(d) => (
                                Protocol::Dhcp,
                                format!(
                                    "DHCP {} {} → {}{}{tag}",
                                    d.kind.name(),
                                    ip.src,
                                    ip.dst,
                                    if d.your_ip.is_unspecified() { String::new() } else { format!(" (address {})", d.your_ip) }
                                ),
                            ),
                            Err(e) => (Protocol::Dhcp, format!("DHCP ({e}){tag}")),
                        },
                        Ok(u) if [u.dst_port, u.src_port].iter().any(|p| [capwap::CONTROL_PORT, capwap::DATA_PORT].contains(p)) => {
                            let port = if [capwap::CONTROL_PORT, capwap::DATA_PORT].contains(&u.dst_port) { u.dst_port } else { u.src_port };
                            (Protocol::Capwap, format!("{} {route}{tag}", capwap::describe(port, &u.payload)))
                        }
                        Ok(u) if u.dst_port == dns::PORT || u.src_port == dns::PORT => match dns::Dns::decode(&u.payload) {
                            Ok(d) if d.response => (
                                Protocol::Dns,
                                format!("DNS answer {} = {}{tag}", d.name, d.answers.first().map_or("not found".to_string(), |a| a.to_string())),
                            ),
                            Ok(d) => (Protocol::Dns, format!("DNS query {} from {}{tag}", d.name, ip.src)),
                            Err(e) => (Protocol::Dns, format!("DNS ({e}){tag}")),
                        },
                        Ok(u) => (Protocol::Udp, format!("UDP {}:{} → {}:{}{tag}", ip.src, u.src_port, ip.dst, u.dst_port)),
                        Err(e) => (Protocol::Udp, format!("UDP ({e}) {route}{tag}")),
                    },
                    PROTO_TCP => match tcp::Tcp::decode(&ip.payload, ip.src, ip.dst) {
                        Ok(s) if !s.payload.is_empty() && (s.dst_port == 80 || s.src_port == 80) => {
                            let first = String::from_utf8_lossy(&s.payload).lines().next().unwrap_or("").chars().take(60).collect::<String>();
                            (Protocol::Http, format!("HTTP {first} {route}{tag}"))
                        }
                        Ok(s) if !s.payload.is_empty() && (s.dst_port == http::TLS_PORT || s.src_port == http::TLS_PORT) => {
                            let first = String::from_utf8_lossy(&s.payload).lines().next().unwrap_or("").chars().take(60).collect::<String>();
                            (Protocol::Http, format!("HTTPS (TLS not simulated) {first} {route}{tag}"))
                        }
                        Ok(s) if !s.payload.is_empty() && (s.dst_port == netconf::PORT || s.src_port == netconf::PORT) => {
                            (Protocol::Tcp, format!("NETCONF {} {route}{tag}", netconf::describe(&s.payload)))
                        }
                        Ok(s) if !s.payload.is_empty() && (s.dst_port == tacacs::TACACS_PORT || s.src_port == tacacs::TACACS_PORT) => {
                            let what = if s.dst_port == tacacs::TACACS_PORT { "authentication START" } else { "authentication REPLY" };
                            (Protocol::Tacacs, format!("TACACS+ {what} (body encrypted) {route}{tag}"))
                        }
                        Ok(s) if !s.payload.is_empty() && [SMTP_PORT, POP3_PORT].iter().any(|p| s.dst_port == *p || s.src_port == *p) => {
                            let first = String::from_utf8_lossy(&s.payload).lines().next().unwrap_or("").chars().take(60).collect::<String>();
                            let first = if first.to_ascii_uppercase().starts_with("PASS ") { "PASS ****".to_string() } else { first };
                            let proto = if s.dst_port == SMTP_PORT || s.src_port == SMTP_PORT { Protocol::Smtp } else { Protocol::Pop3 };
                            (proto, format!("{} {first} {route}{tag}", proto.name()))
                        }
                        Ok(s) if !s.payload.is_empty() && (s.dst_port == FTP_PORT || s.src_port == FTP_PORT) => {
                            let first = String::from_utf8_lossy(&s.payload).lines().next().unwrap_or("").chars().take(60).collect::<String>();
                            // Never show the password a client sends.
                            let first = if first.to_ascii_uppercase().starts_with("PASS ") { "PASS ****".to_string() } else { first };
                            (Protocol::Ftp, format!("FTP {first} {route}{tag}"))
                        }
                        Ok(s) if !s.payload.is_empty() && (FTP_DATA_PORTS.contains(&s.dst_port) || FTP_DATA_PORTS.contains(&s.src_port)) => {
                            (Protocol::Ftp, format!("FTP data, {} bytes {route}{tag}", s.payload.len()))
                        }
                        Ok(s) => (Protocol::Tcp, format!("TCP [{}] {}:{} → {}:{}{tag}", s.flag_names(), ip.src, s.src_port, ip.dst, s.dst_port)),
                        Err(e) => (Protocol::Tcp, format!("TCP ({e}) {route}{tag}")),
                    },
                    vpn::PROTO_GRE => match vpn::Gre::decode(&ip.payload) {
                        Ok(g) if g.protocol == ETHERTYPE_IPV4 && depth < 4 => {
                            let inner = Frame { dst: f.dst, src: f.src, vlan: None, ethertype: ETHERTYPE_IPV4, payload: g.payload }.encode();
                            let (_, what) = summarize_depth(&inner, depth + 1);
                            (Protocol::Gre, format!("GRE {route} carrying {what}{tag}"))
                        }
                        Ok(g) => (Protocol::Gre, format!("GRE {route} carrying protocol type 0x{:04x}{tag}", g.protocol)),
                        Err(e) => (Protocol::Gre, format!("GRE ({e}) {route}{tag}")),
                    },
                    vpn::PROTO_ESP => match vpn::Esp::decode(&ip.payload) {
                        Ok(e) => (Protocol::Esp, format!("ESP SPI 0x{:08x} seq {} {route}, {} bytes protected{tag}", e.spi, e.seq, e.data.len())),
                        Err(e) => (Protocol::Esp, format!("ESP ({e}) {route}{tag}")),
                    },
                    p => (Protocol::Ipv4, format!("IPv4 protocol {p} {route}{tag}")),
                }
            }
            Err(e) => (Protocol::Ipv4, format!("IPv4 ({e}){tag}")),
        },
        ipv6::ETHERTYPE_IPV6 => match ipv6::Ipv6::decode(&f.payload) {
            Ok(ip) if ip.next_header == PROTO_UDP => match dhcp6::udp6_decode(ip.src, ip.dst, &ip.payload) {
                Ok((_, dp, body))
                    if [dhcp6::CLIENT_PORT, dhcp6::SERVER_PORT].contains(&dp)
                        && let Some(r) = dhcp6::Relay::decode(&body) =>
                {
                    match r {
                        Ok(r) => (
                            Protocol::Dhcp,
                            format!(
                                "DHCPv6 {} ({}) {} → {}{tag}",
                                if r.reply { "Relay-reply" } else { "Relay-forward" },
                                dhcp6::Dhcp6::decode(&r.message).map_or("?", |m| m.kind.name()),
                                ip.src,
                                ip.dst
                            ),
                        ),
                        Err(e) => (Protocol::Dhcp, format!("DHCPv6 relay ({e}){tag}")),
                    }
                }
                Ok((_, dp, body)) if [dhcp6::CLIENT_PORT, dhcp6::SERVER_PORT].contains(&dp) => match dhcp6::Dhcp6::decode(&body) {
                    Ok(m) => (
                        Protocol::Dhcp,
                        format!(
                            "DHCPv6 {} {} → {}{}{tag}",
                            m.kind.name(),
                            ip.src,
                            ip.dst,
                            m.ia.and_then(|(_, a)| a).map(|a| format!(" (address {a})")).unwrap_or_default()
                        ),
                    ),
                    Err(e) => (Protocol::Dhcp, format!("DHCPv6 ({e}){tag}")),
                },
                Ok((sp, dp, _)) => (Protocol::Ipv6, format!("UDP [{}]:{sp} → [{}]:{dp}{tag}", ip.src, ip.dst)),
                Err(e) => (Protocol::Ipv6, format!("UDP ({e}){tag}")),
            },
            Ok(ip) if ip.next_header == ospf6::PROTO_OSPF => match ospf6::Hello6::decode(ip.src, ip.dst, &ip.payload) {
                Ok(h) => (
                    Protocol::Ospf,
                    format!(
                        "OSPFv3 hello from router {} ({} neighbour{}) {} → {}{tag}",
                        h.router_id,
                        h.neighbors.len(),
                        if h.neighbors.len() == 1 { "" } else { "s" },
                        ip.src,
                        ip.dst
                    ),
                ),
                Err(e) => (Protocol::Ospf, format!("OSPFv3 ({e}){tag}")),
            },
            Ok(ip) => {
                let (ndp, text) = ipv6::describe(&ip);
                let proto = if ndp {
                    Protocol::Ndp
                } else if ip.next_header == ipv6::PROTO_ICMPV6 {
                    Protocol::Icmpv6
                } else {
                    Protocol::Ipv6
                };
                (proto, format!("{text}{tag}"))
            }
            Err(e) => (Protocol::Ipv6, format!("IPv6 ({e}){tag}")),
        },
        lacp::ETHERTYPE_SLOW => match lacp::Lacpdu::decode(&f.payload) {
            Ok(p) => {
                let state = |s: u8| {
                    let mut v = vec![if s & lacp::ACTIVITY != 0 { "active" } else { "passive" }];
                    if s & lacp::SYNC != 0 {
                        v.push("in sync");
                    }
                    if s & lacp::DEFAULTED != 0 {
                        v.push("no partner yet");
                    }
                    v.join(", ")
                };
                (Protocol::Lacp, format!("LACP port {} key {} ({}){tag}", p.actor.port, p.actor.key, state(p.actor.state)))
            }
            Err(e) => (Protocol::Lacp, format!("LACP ({e}){tag}")),
        },
        lldp::ETHERTYPE_LLDP => match lldp::Lldp::decode(&f.payload) {
            Ok(l) => (
                Protocol::Lldp,
                format!("LLDP {} on {}, keep {} s{tag}", if l.system_name.is_empty() { l.chassis.to_string() } else { l.system_name }, l.port, l.ttl),
            ),
            Err(e) => (Protocol::Lldp, format!("LLDP ({e}){tag}")),
        },
        wlan::ETHERTYPE_EAPOL => match wlan::Eapol::decode(&f.payload) {
            Ok(e) => (Protocol::Eapol, format!("{}{tag}", e.describe())),
            Err(e) => (Protocol::Eapol, format!("EAPOL ({e}){tag}")),
        },
        len if len <= 1_500 && f.payload.starts_with(&[0x42, 0x42, 0x03]) => match control::Bpdu::decode(&f.payload) {
            Ok(b) => (Protocol::Stp, format!("STP BPDU from {}: root {}, cost {}{tag}", b.bridge, b.root, b.root_cost)),
            Err(e) => (Protocol::Stp, format!("STP ({e}){tag}")),
        },
        other => (Protocol::Other, format!("ethertype 0x{other:04x}{tag}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(n: u8) -> MacAddr {
        MacAddr([2, 0, 0, 0, 0, n])
    }

    #[test]
    fn checksum_matches_rfc1071_example() {
        // RFC 1071 §3 worked example: 00 01 f2 03 f4 f5 f6 f7 sums to 0xddf2.
        assert_eq!(checksum(&[&[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]]), !0xddf2);
        // Odd split across chunks gives the same result as one chunk.
        assert_eq!(checksum(&[&[0x00, 0x01, 0xf2], &[0x03, 0xf4, 0xf5, 0xf6, 0xf7]]), !0xddf2);
    }

    #[test]
    fn tagged_frame_round_trips_and_pads() {
        let f = Frame { dst: MacAddr::BROADCAST, src: mac(1), vlan: Some(10), ethertype: ETHERTYPE_ARP, payload: vec![1, 2, 3] };
        let b = f.encode();
        assert_eq!(b.len(), 18 + 46);
        let d = Frame::decode(&b).unwrap();
        assert_eq!((d.dst, d.src, d.vlan, d.ethertype), (f.dst, f.src, Some(10), ETHERTYPE_ARP));
        assert_eq!(&d.payload[..3], &[1, 2, 3]);
        assert!(Frame::decode(&b[..13]).is_err());
        assert!(Frame::decode(&b[..16]).is_err());
    }

    #[test]
    fn arp_round_trips() {
        let a = Arp {
            op: ArpOp::Reply,
            sender_mac: mac(1),
            sender_ip: Ipv4Addr::new(10, 0, 0, 1),
            target_mac: mac(2),
            target_ip: Ipv4Addr::new(10, 0, 0, 2),
        };
        assert_eq!(Arp::decode(&a.encode()).unwrap(), a);
        assert!(Arp::decode(&a.encode()[..20]).is_err());
    }

    #[test]
    fn ipv4_icmp_round_trip_through_a_padded_frame() {
        let icmp = Icmp::EchoRequest { id: 7, seq: 3, data: vec![0xab; 4] };
        let ip =
            Ipv4 { id: 1, ttl: 64, protocol: PROTO_ICMP, src: Ipv4Addr::new(10, 0, 0, 1), dst: Ipv4Addr::new(10, 0, 0, 2), payload: icmp.encode() };
        let frame = Frame { dst: mac(2), src: mac(1), vlan: None, ethertype: ETHERTYPE_IPV4, payload: ip.encode() }.encode();
        // Padding past the IPv4 total length is ignored.
        let back = Ipv4::decode(&Frame::decode(&frame).unwrap().payload).unwrap();
        assert_eq!(back, ip);
        assert_eq!(Icmp::decode(&back.payload).unwrap(), icmp);
        let (proto, text) = summarize(&frame);
        assert_eq!(proto, Protocol::Icmp);
        assert_eq!(text, "ICMP echo request seq 3 10.0.0.1 → 10.0.0.2 ttl 64");
    }

    #[test]
    fn corrupted_headers_are_rejected() {
        let ip = Ipv4 { id: 1, ttl: 64, protocol: PROTO_ICMP, src: Ipv4Addr::new(1, 1, 1, 1), dst: Ipv4Addr::new(2, 2, 2, 2), payload: vec![] };
        let mut b = ip.encode();
        b[8] ^= 1;
        assert_eq!(Ipv4::decode(&b), Err(WireError::Checksum("ipv4")));
        let mut b = ip.encode();
        b[2] = 0xff; // total length beyond the buffer
        assert!(Ipv4::decode(&b).is_err());
        let mut b = ip.encode();
        b[0] = 0x41; // IHL 4
        assert!(Ipv4::decode(&b).is_err());
        let mut icmp = Icmp::EchoReply { id: 1, seq: 1, data: vec![] }.encode();
        icmp[5] ^= 1;
        assert_eq!(Icmp::decode(&icmp), Err(WireError::Checksum("icmp")));
    }

    #[test]
    fn error_messages_quote_the_echo() {
        let echo = Ipv4 {
            id: 9,
            ttl: 1,
            protocol: PROTO_ICMP,
            src: Ipv4Addr::new(1, 1, 1, 1),
            dst: Ipv4Addr::new(2, 2, 2, 2),
            payload: Icmp::EchoRequest { id: 42, seq: 5, data: vec![0; 32] }.encode(),
        }
        .encode();
        let quoted = Icmp::quote(&echo);
        assert_eq!(quoted.len(), 28);
        let te = Icmp::decode(&Icmp::TimeExceeded { original: quoted }.encode()).unwrap();
        let Icmp::TimeExceeded { original } = te else { panic!("not time exceeded") };
        assert_eq!(Icmp::quoted_echo(&original), Some((42, 5)));
        assert_eq!(Icmp::quoted_echo(&[0x45]), None);
    }

    #[test]
    fn udp_checksum_covers_the_pseudo_header() {
        let (s, d) = (Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 255));
        let u = Udp { src_port: 520, dst_port: 520, payload: vec![1, 2, 3] };
        let b = u.encode(s, d);
        assert_eq!(Udp::decode(&b, s, d).unwrap(), u);
        assert_eq!(Udp::decode(&b, s, Ipv4Addr::new(10, 0, 0, 254)), Err(WireError::Checksum("udp")));
    }

    #[test]
    fn eigrp_hellos_are_summarized_and_survive_truncation() {
        let ip = Ipv4 {
            id: 0,
            ttl: 1,
            protocol: eigrp::PROTO_EIGRP,
            src: Ipv4Addr::new(10, 0, 12, 1),
            dst: eigrp::EIGRP_ROUTERS,
            payload: eigrp::EigrpHello::new(7).encode(),
        };
        let frame = Frame {
            dst: control::ipv4_multicast_mac(eigrp::EIGRP_ROUTERS),
            src: mac(1),
            vlan: None,
            ethertype: ETHERTYPE_IPV4,
            payload: ip.encode(),
        }
        .encode();
        let (proto, text) = summarize(&frame);
        assert_eq!(proto, Protocol::Eigrp);
        assert_eq!(text, "EIGRP hello, AS 7 (hold 15 s) 10.0.12.1 → 224.0.0.10");
        for n in 0..frame.len() {
            let _ = summarize(&frame[..n]);
        }
    }

    #[test]
    fn decoders_survive_every_truncation() {
        let ip = Ipv4 {
            id: 1,
            ttl: 64,
            protocol: PROTO_ICMP,
            src: Ipv4Addr::new(1, 1, 1, 1),
            dst: Ipv4Addr::new(2, 2, 2, 2),
            payload: Icmp::EchoRequest { id: 1, seq: 1, data: vec![7; 20] }.encode(),
        };
        let frame = Frame { dst: mac(2), src: mac(1), vlan: Some(5), ethertype: ETHERTYPE_IPV4, payload: ip.encode() }.encode();
        for n in 0..frame.len() {
            let _ = summarize(&frame[..n]);
            let _ = Ipv4::decode(&frame[..n]);
            let _ = Icmp::decode(&frame[..n]);
            let _ = Arp::decode(&frame[..n]);
            let _ = Udp::decode(&frame[..n], ip.src, ip.dst);
        }
    }
}
