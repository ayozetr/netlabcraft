//! The end-device firewall in the host input path (`netlabcraft_model::hostfw`). A packet that
//! reaches an end device is looked at before the device acts on it: an ICMP echo request, the
//! first segment of a TCP connection (SYN without ACK) and a UDP datagram are matched against the
//! rule list of its IP version. Echo replies, the rest of a TCP connection, ICMP errors, neighbour
//! discovery and the answers to DHCP and DNS requests always pass, so a device with its firewall
//! on can still use the network. A dropped packet gets no answer at all.

use std::net::IpAddr;

use netlabcraft_model::Device;
use netlabcraft_model::hostfw::Inbound;
use netlabcraft_wire::ipv6::{Ipv6, PROTO_ICMPV6};
use netlabcraft_wire::{Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP};

/// The UDP ports of DHCP clients (68, 546) and DNS servers (53): answers come from or go to them.
const DHCP_CLIENT_PORTS: [u16; 2] = [68, 546];
const DNS_PORT: u16 = 53;
/// ICMP echo request types (IPv4 and IPv6).
const ICMP_ECHO_REQUEST: u8 = 8;
const ICMPV6_ECHO_REQUEST: u8 = 128;
const TCP_SYN: u8 = 0x02;
const TCP_ACK: u8 = 0x10;

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

/// What a transport payload is as far as the firewall cares: the kind of inbound packet and the
/// local port, or `None` for what always passes.
fn classify(protocol: u8, v6: bool, payload: &[u8]) -> Option<(Inbound, Option<u16>)> {
    match protocol {
        PROTO_ICMP if !v6 && payload.first() == Some(&ICMP_ECHO_REQUEST) => Some((Inbound::Icmp, None)),
        PROTO_ICMPV6 if v6 && payload.first() == Some(&ICMPV6_ECHO_REQUEST) => Some((Inbound::Icmp, None)),
        PROTO_TCP => {
            let flags = *payload.get(13)?;
            (flags & TCP_SYN != 0 && flags & TCP_ACK == 0).then_some((Inbound::Tcp, be16(payload, 2)))
        }
        PROTO_UDP => {
            let (src, dst) = (be16(payload, 0)?, be16(payload, 2)?);
            if src == DNS_PORT || DHCP_CLIENT_PORTS.contains(&dst) {
                return None;
            }
            Some((Inbound::Udp, Some(dst)))
        }
        _ => None,
    }
}

/// Whether the firewall of `d` drops this inbound IPv4 packet.
pub(crate) fn blocks4(d: &Device, ip: &Ipv4) -> bool {
    if !d.kind.is_host() || !d.host_fw.v4.on {
        return false;
    }
    match classify(ip.protocol, false, &ip.payload) {
        Some((kind, port)) => !d.host_fw.v4.permits(kind, IpAddr::V4(ip.src), port),
        None => false,
    }
}

/// Whether the firewall of `d` drops this inbound IPv6 packet (multicast is never filtered).
pub(crate) fn blocks6(d: &Device, ip: &Ipv6) -> bool {
    if !d.kind.is_host() || !d.host_fw.v6.on || ip.dst.is_multicast() {
        return false;
    }
    match classify(ip.next_header, true, &ip.payload) {
        Some((kind, port)) => !d.host_fw.v6.permits(kind, IpAddr::V6(ip.src), port),
        None => false,
    }
}
