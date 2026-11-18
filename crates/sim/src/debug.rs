//! The `debug` framework: a per-device set of flags and the one way a protocol reports
//! through them.
//!
//! A protocol that wants a `debug` topic does four things:
//! 1. adds a variant to [`DebugKind`], its row to [`DebugKind::info`] and to [`DebugKind::ALL`];
//! 2. adds the grammar rows for `debug …` and `no debug …` in `crates/ios/src/grammar.rs`, and the
//!    topic to the table in `crates/ios/src/exec/debug.rs`;
//! 3. calls [`Sim::debug_log`] where the protocol event happens (guarded by [`Sim::debug_on`]
//!    when building the message is not free);
//! 4. tests the message through `show logging` and the flag through `show debugging`.
//!
//! Messages are stored as `[HH:MM:SS] LABEL debug: text`; they always have severity 7
//! (debugging), so `logging console`, `logging buffered`, `logging monitor` and `logging trap`
//! filter them. The buffer, the console queue and the monitor queue each hold at most
//! [`crate::MAX_LOG`] messages (the oldest are dropped), so a chatty debug never grows memory.

use netlabcraft_core::DeviceId;

use crate::{DevState, Sim};

/// One topic a person can turn on with `debug …`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DebugKind {
    /// `debug ip icmp`: echo messages this device receives.
    Icmp,
    /// `debug ip rip`: RIP updates this router hears.
    Rip,
    /// `debug ip rip events`: routes the RIP process learns, refreshes and drops.
    RipEvents,
    /// `debug ip ospf hello`: the hellos this router hears.
    OspfHello,
    /// `debug ip ospf adj`: each neighbour's walk through the adjacency states.
    OspfAdj,
    /// `debug ip ospf events`: interface, DR election, SPF and timer events.
    OspfEvents,
    /// `debug ip ospf lsa-generation`: the LSAs this router originates and flushes.
    OspfLsa,
    /// `debug ip ospf flood`: LSAs sent and received in updates.
    OspfFlood,
    /// `debug ip ospf packet`: every OSPF packet this router hears.
    OspfPacket,
    /// `debug eigrp packets`: the EIGRP packets this router hears.
    Eigrp,
    /// `debug eigrp fsm`: route computations (feasible successors).
    EigrpFsm,
    /// `debug eigrp neighbors`: neighbours coming and going.
    EigrpNeighbors,
    /// `debug ip bgp`: the messages a BGP speaker hears and its session changes.
    Bgp,
    /// `debug ip packet [ACL]`: each packet this device routes.
    Packet,
    /// `debug ip nat`: new translations.
    Nat,
    /// `debug ip nat detailed`: translations with the port and the table they came from.
    NatDetailed,
    /// `debug ip dhcp server events`: addresses handed out and taken back.
    Dhcp,
    /// `debug arp`: ARP requests and replies.
    Arp,
    /// `debug spanning-tree events`: port role and state changes.
    StpEvents,
    /// `debug spanning-tree bpdu`: BPDUs received.
    StpBpdu,
    /// `debug spanning-tree root`: the root bridge changing.
    StpRoot,
    /// `debug cdp packets`: CDP advertisements received.
    CdpPackets,
    /// `debug cdp events`: neighbours added and aged out.
    CdpEvents,
    /// `debug lldp`: LLDP advertisements received.
    Lldp,
    /// `debug ppp negotiation`: LCP and IPCP exchanges.
    PppNegotiation,
    /// `debug ppp authentication`: PAP and CHAP.
    PppAuth,
    /// `debug ppp packet`: every PPP frame.
    PppPacket,
    /// `debug pppoe events`: discovery and session changes.
    PppoeEvents,
    /// `debug pppoe packets`: discovery and session frames.
    PppoePackets,
    /// `debug ip http`: requests to the device's web server.
    Http,
    /// `debug standby`: HSRP hellos and state changes.
    Standby,
    /// `debug vrrp`: VRRP advertisements and state changes.
    Vrrp,
    /// `debug ip routing`: routes added and removed.
    Routing,
    /// `debug ntp`: NTP exchanges and clock changes.
    Ntp,
    /// `debug snmp`: SNMP requests and traps.
    Snmp,
    /// `debug dot1x`: 802.1X port authentication.
    Dot1x,
    /// `debug ip ssh`: SSH logins.
    Ssh,
    /// `debug crypto isakmp`: IKE phase 1 and 2 exchanges.
    CryptoIsakmp,
    /// `debug crypto ipsec`: IPsec SAs coming and going.
    CryptoIpsec,
}

impl DebugKind {
    /// Every topic, in the order `show debugging` lists them. Add a new variant here too.
    pub const ALL: &'static [DebugKind] = &[
        DebugKind::Icmp,
        DebugKind::Rip,
        DebugKind::RipEvents,
        DebugKind::OspfHello,
        DebugKind::OspfAdj,
        DebugKind::OspfEvents,
        DebugKind::OspfLsa,
        DebugKind::OspfFlood,
        DebugKind::OspfPacket,
        DebugKind::Eigrp,
        DebugKind::EigrpFsm,
        DebugKind::EigrpNeighbors,
        DebugKind::Bgp,
        DebugKind::Packet,
        DebugKind::Nat,
        DebugKind::NatDetailed,
        DebugKind::Dhcp,
        DebugKind::Arp,
        DebugKind::StpEvents,
        DebugKind::StpBpdu,
        DebugKind::StpRoot,
        DebugKind::CdpPackets,
        DebugKind::CdpEvents,
        DebugKind::Lldp,
        DebugKind::PppNegotiation,
        DebugKind::PppAuth,
        DebugKind::PppPacket,
        DebugKind::PppoeEvents,
        DebugKind::PppoePackets,
        DebugKind::Http,
        DebugKind::Standby,
        DebugKind::Vrrp,
        DebugKind::Routing,
        DebugKind::Ntp,
        DebugKind::Snmp,
        DebugKind::Dot1x,
        DebugKind::Ssh,
        DebugKind::CryptoIsakmp,
        DebugKind::CryptoIpsec,
    ];

    /// `(label, name)`: the label starts every message of the topic (`LABEL debug: text`), the
    /// name is what `show debugging` calls it.
    ///
    /// The two OSPF families that already printed their own prefix keep it: their messages are
    /// written whole by the caller through [`Sim::debug_raw`].
    pub fn info(self) -> (&'static str, &'static str) {
        match self {
            DebugKind::Icmp => ("ICMP", "ICMP"),
            DebugKind::Rip => ("RIP", "RIP"),
            DebugKind::RipEvents => ("RIP event", "RIP events"),
            DebugKind::OspfHello => ("OSPF hello", "OSPF hellos"),
            DebugKind::OspfAdj => ("OSPF adjacency", "OSPF adjacencies"),
            DebugKind::OspfEvents => ("OSPF event", "OSPF events"),
            DebugKind::OspfLsa => ("OSPF LSA", "OSPF LSA generation"),
            DebugKind::OspfFlood => ("OSPF flood", "OSPF flooding"),
            DebugKind::OspfPacket => ("OSPF packet", "OSPF packets"),
            DebugKind::Eigrp => ("EIGRP", "EIGRP"),
            DebugKind::EigrpFsm => ("EIGRP route", "EIGRP route computations"),
            DebugKind::EigrpNeighbors => ("EIGRP neighbour", "EIGRP neighbours"),
            DebugKind::Bgp => ("BGP", "BGP"),
            DebugKind::Packet => ("IP", "IP packets"),
            DebugKind::Nat => ("NAT", "NAT"),
            DebugKind::NatDetailed => ("NAT detail", "NAT in detail"),
            DebugKind::Dhcp => ("DHCP", "DHCP server"),
            DebugKind::Arp => ("ARP", "ARP"),
            DebugKind::StpEvents => ("STP event", "spanning-tree events"),
            DebugKind::StpBpdu => ("STP BPDU", "spanning-tree BPDUs"),
            DebugKind::StpRoot => ("STP root", "spanning-tree root changes"),
            DebugKind::CdpPackets => ("CDP packet", "CDP packets"),
            DebugKind::CdpEvents => ("CDP event", "CDP events"),
            DebugKind::Lldp => ("LLDP", "LLDP"),
            DebugKind::PppNegotiation => ("PPP negotiation", "PPP negotiation"),
            DebugKind::PppAuth => ("PPP authentication", "PPP authentication"),
            DebugKind::PppPacket => ("PPP packet", "PPP packets"),
            DebugKind::PppoeEvents => ("PPPoE event", "PPPoE events"),
            DebugKind::PppoePackets => ("PPPoE packet", "PPPoE packets"),
            DebugKind::Http => ("HTTP", "HTTP server"),
            DebugKind::Standby => ("HSRP", "HSRP"),
            DebugKind::Vrrp => ("VRRP", "VRRP"),
            DebugKind::Routing => ("Routing", "IP routing"),
            DebugKind::Ntp => ("NTP", "NTP"),
            DebugKind::Snmp => ("SNMP", "SNMP"),
            DebugKind::Dot1x => ("802.1X", "802.1X"),
            DebugKind::Ssh => ("SSH", "SSH"),
            DebugKind::CryptoIsakmp => ("ISAKMP", "ISAKMP"),
            DebugKind::CryptoIpsec => ("IPsec", "IPsec"),
        }
    }

    /// The label that starts this topic's messages.
    pub fn label(self) -> &'static str {
        self.info().0
    }

    /// What `show debugging` calls this topic.
    pub fn name(self) -> &'static str {
        self.info().1
    }
}

impl DevState {
    /// Is this `debug` topic on for the device?
    pub fn debug_on(&self, kind: DebugKind) -> bool {
        self.debug.contains(&kind)
    }
}

impl Sim {
    /// Is this `debug` topic on for the device?
    pub fn debug_on(&self, d: DeviceId, kind: DebugKind) -> bool {
        self.state.get(&d).is_some_and(|s| s.debug_on(kind))
    }

    /// Turns one `debug` topic on or off. Turning off one that is not on does nothing.
    pub fn set_debug(&mut self, d: DeviceId, kind: DebugKind, on: bool) {
        let st = self.st(d);
        if on {
            st.debug.insert(kind);
        } else {
            st.debug.remove(&kind);
            if kind == DebugKind::Packet {
                st.debug_packet_acl = None;
            }
        }
    }

    /// `debug ip packet ACL`: report only the packets this access list permits (none: all).
    pub fn set_debug_packet_acl(&mut self, d: DeviceId, acl: Option<String>) {
        self.st(d).debug_packet_acl = acl;
    }

    /// Should `debug ip packet` report this packet on this device? On, and permitted by the access
    /// list when one was named (a list that no longer exists reports nothing).
    pub(crate) fn debug_packet_wanted(&self, d: &netlabcraft_model::Device, ip: &netlabcraft_wire::Ipv4) -> bool {
        let Some(s) = self.state.get(&d.id) else { return false };
        if !s.debug_on(DebugKind::Packet) {
            return false;
        }
        match &s.debug_packet_acl {
            None => true,
            Some(name) => d.acls.iter().find(|a| &a.name == name).is_some_and(|a| a.evaluate(&crate::acl_key(ip)).0),
        }
    }

    /// `undebug all`: every topic off.
    pub fn clear_debug(&mut self, d: DeviceId) {
        if let Some(s) = self.state.get_mut(&d) {
            s.debug.clear();
            s.debug_packet_acl = None;
        }
    }

    /// The topics that are on, in [`DebugKind::ALL`] order.
    pub fn debug_kinds(&self, d: DeviceId) -> Vec<DebugKind> {
        let Some(s) = self.state.get(&d) else { return Vec::new() };
        DebugKind::ALL.iter().copied().filter(|k| s.debug_on(*k)).collect()
    }

    /// The topics that are on, by the name `show debugging` prints.
    pub fn debugs(&self, d: DeviceId) -> Vec<&'static str> {
        self.debug_kinds(d).into_iter().map(DebugKind::name).collect()
    }

    /// Reports a protocol event when its topic is on: `LABEL debug: text` goes to the device's
    /// logging buffer, console and monitor sessions, at severity 7.
    pub fn debug_log(&mut self, d: DeviceId, kind: DebugKind, text: &str) {
        if self.debug_on(d, kind) {
            let msg = format!("{} debug: {text}", kind.label());
            self.log_at(d, &msg, 7);
        }
    }

    /// Like [`Sim::debug_log`], for a message the caller worded whole (it prints its own prefix):
    /// still only when the topic is on, still severity 7.
    pub fn debug_raw(&mut self, d: DeviceId, kind: DebugKind, msg: &str) {
        if self.debug_on(d, kind) {
            self.log_at(d, msg, 7);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_listed_once_with_a_label() {
        let mut seen = std::collections::BTreeSet::new();
        for k in DebugKind::ALL {
            assert!(seen.insert(*k), "{k:?} is listed twice");
            assert!(!k.label().is_empty() && !k.name().is_empty());
        }
    }
}
