//! What a router's DHCP server counts and notices: messages by type, address conflicts (an
//! address of a pool some other device already uses, found when choosing what to offer), and
//! the `debug ip dhcp server packet` trace.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::dhcp::DhcpKind;

use crate::Sim;

/// Most conflicts remembered.
pub const MAX_CONFLICTS: usize = 256;

/// Message counts and conflicts of one DHCP server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerStats {
    pub received: BTreeMap<&'static str, u64>,
    pub sent: BTreeMap<&'static str, u64>,
    /// Address → simulated µs when it was found in use.
    pub conflicts: BTreeMap<Ipv4Addr, u64>,
    /// `debug ip dhcp server packet`.
    pub debug_packet: bool,
}

/// The name of a message type, as the statistics list it.
pub fn kind_name(k: DhcpKind) -> &'static str {
    match k {
        DhcpKind::Discover => "DISCOVER",
        DhcpKind::Offer => "OFFER",
        DhcpKind::Request => "REQUEST",
        DhcpKind::Ack => "ACK",
        DhcpKind::Nak => "NAK",
        DhcpKind::Release => "RELEASE",
    }
}

impl Sim {
    /// Counts a message the server received, and traces it when debugging.
    pub(crate) fn dhcp_srv_in(&mut self, d: DeviceId, kind: DhcpKind, mac: MacAddr, iface: &str) {
        let now_debug = {
            let st = &mut self.st(d).dhcp_srv;
            *st.received.entry(kind_name(kind)).or_insert(0) += 1;
            st.debug_packet
        };
        if now_debug {
            self.log(d, &format!("DHCP packet debug: {} from {mac} arrived on {iface}", kind_name(kind)));
        }
    }

    /// Counts a message the server sent, and traces it when debugging.
    pub(crate) fn dhcp_srv_out(&mut self, d: DeviceId, kind: DhcpKind, mac: MacAddr, addr: Ipv4Addr) {
        let now_debug = {
            let st = &mut self.st(d).dhcp_srv;
            *st.sent.entry(kind_name(kind)).or_insert(0) += 1;
            st.debug_packet
        };
        if now_debug {
            let what = if addr.is_unspecified() { String::new() } else { format!(" for {addr}") };
            self.log(d, &format!("DHCP packet debug: {} to {mac}{what}", kind_name(kind)));
        }
    }

    /// Notes the pool addresses below `picked` that another device already uses: the server
    /// passed over them.
    pub(crate) fn dhcp_note_conflicts(&mut self, t: &Topology, d: &Device, net: netlabcraft_core::Ipv4Net, picked: Ipv4Addr, mac: MacAddr) {
        let mine: Vec<Ipv4Addr> = crate::l3_ifaces(d).iter().map(|i| i.net.addr).collect();
        let excluded = |ip: Ipv4Addr| d.dhcp_excluded.iter().any(|(a, b)| (u32::from(*a)..=u32::from(*b)).contains(&u32::from(ip)));
        let leases: Vec<Ipv4Addr> = self.state.get(&d.id).map(|s| s.leases.keys().copied().collect()).unwrap_or_default();
        let used: Vec<Ipv4Addr> = t
            .devices
            .iter()
            .flat_map(|x| {
                x.ports.iter().filter(|p| p.mac != mac).filter_map(|p| p.ip.map(|n| n.addr)).chain(x.svis.iter().filter_map(|s| s.ip.map(|n| n.addr)))
            })
            .filter(|ip| net.contains(*ip) && u32::from(*ip) < u32::from(picked) && !mine.contains(ip) && !excluded(*ip) && !leases.contains(ip))
            .collect();
        let now = self.now;
        let st = &mut self.st(d.id).dhcp_srv;
        for ip in used {
            if st.conflicts.len() >= MAX_CONFLICTS {
                break;
            }
            st.conflicts.entry(ip).or_insert(now);
        }
    }

    /// `debug ip dhcp server packet` on or off.
    pub fn set_dhcp_packet_debug(&mut self, d: DeviceId, on: bool) {
        self.st(d).dhcp_srv.debug_packet = on;
    }

    /// `clear ip dhcp conflict *`.
    pub fn clear_dhcp_conflicts(&mut self, d: DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.dhcp_srv.conflicts.clear();
        }
    }

    /// `clear ip dhcp server statistics`.
    pub fn clear_dhcp_stats(&mut self, d: DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.dhcp_srv.received.clear();
            st.dhcp_srv.sent.clear();
        }
    }

    /// Forgets one lease (`clear ip dhcp binding A.B.C.D`); whether there was one.
    pub fn clear_lease(&mut self, d: DeviceId, ip: Ipv4Addr) -> bool {
        self.state.get_mut(&d).is_some_and(|st| st.leases.remove(&ip).is_some())
    }
}
