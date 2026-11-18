//! `debug`, `no debug`, `undebug all` and `show debugging`: the console side of the debug
//! framework in `netlabcraft_sim::debug`.
//!
//! [`TOPICS`] maps the words after `debug` to the flag they turn on. A protocol adds a row there
//! (and its grammar rows in `grammar.rs`); the flag's own wording lives next to the event in the
//! simulator.

use netlabcraft_sim::DebugKind;

use super::{Ctx, R};
use crate::grammar::{Id, Spec, Tok};

/// `(words after "debug", flag, what is reported)`.
pub(crate) const TOPICS: &[(&str, DebugKind, &str)] = &[
    ("ip icmp", DebugKind::Icmp, "Report echo requests and replies this device receives"),
    ("ip rip", DebugKind::Rip, "Report the RIP updates this router hears"),
    ("ip rip events", DebugKind::RipEvents, "Report the routes RIP learns, refreshes and drops"),
    ("ip packet", DebugKind::Packet, "Report each packet this device routes"),
    ("ip packet", DebugKind::Packet, "Report the routed packets an access list permits"),
    ("ip nat", DebugKind::Nat, "Report new NAT translations"),
    ("ip nat detailed", DebugKind::NatDetailed, "Report NAT translations with their ports and tables"),
    ("ip dhcp server events", DebugKind::Dhcp, "Report the addresses this DHCP server hands out"),
    ("eigrp packets", DebugKind::Eigrp, "Report the EIGRP packets this router hears"),
    ("eigrp fsm", DebugKind::EigrpFsm, "Report the EIGRP route computations"),
    ("eigrp neighbors", DebugKind::EigrpNeighbors, "Report EIGRP neighbours coming and going"),
    ("arp", DebugKind::Arp, "Report the ARP requests and replies this device handles"),
    ("spanning-tree events", DebugKind::StpEvents, "Report spanning-tree port role and state changes"),
    ("spanning-tree bpdu", DebugKind::StpBpdu, "Report the BPDUs this device receives"),
    ("spanning-tree root", DebugKind::StpRoot, "Report the root bridge changing"),
    ("cdp packets", DebugKind::CdpPackets, "Report the CDP advertisements this device receives"),
    ("cdp events", DebugKind::CdpEvents, "Report CDP neighbours added and aged out"),
    ("lldp", DebugKind::Lldp, "Report the LLDP advertisements this device receives"),
    ("ppp negotiation", DebugKind::PppNegotiation, "Report PPP link and address negotiation"),
    ("ppp authentication", DebugKind::PppAuth, "Report PPP PAP and CHAP exchanges"),
    ("ppp packet", DebugKind::PppPacket, "Report every PPP frame"),
    ("pppoe events", DebugKind::PppoeEvents, "Report PPPoE discovery and session changes"),
    ("pppoe packets", DebugKind::PppoePackets, "Report the PPPoE frames this device handles"),
    ("ip http", DebugKind::Http, "Report the requests this device's web server handles"),
    ("standby", DebugKind::Standby, "Report HSRP hellos and state changes"),
    ("vrrp", DebugKind::Vrrp, "Report VRRP advertisements and state changes"),
    ("ip routing", DebugKind::Routing, "Report routes being added and removed"),
    ("ntp events", DebugKind::Ntp, "Report NTP exchanges and clock changes"),
    ("ntp packets", DebugKind::Ntp, "Report NTP exchanges and clock changes"),
    ("snmp packets", DebugKind::Snmp, "Report SNMP requests, answers and traps"),
    ("dot1x events", DebugKind::Dot1x, "Report 802.1X port authentication"),
    ("dot1x packets", DebugKind::Dot1x, "Report 802.1X port authentication"),
    ("ip ssh", DebugKind::Ssh, "Report SSH logins to this device"),
    ("crypto isakmp", DebugKind::CryptoIsakmp, "Report the IKE exchanges of this device"),
    ("crypto ipsec", DebugKind::CryptoIpsec, "Report IPsec SAs coming and going"),
    ("crypto pki", DebugKind::CryptoPki, "Report certificates checked, requests, grants and revocations"),
    ("ip policy", DebugKind::Policy, "Report the decisions of policy-based routing"),
    ("ip pim", DebugKind::Pim, "Report the PIM hellos, joins and registers this router hears"),
    ("ip mpacket", DebugKind::Mpacket, "Report what this router does with each multicast data packet"),
];

/// The words of a spec after `[no] debug`, joined by spaces, without a trailing argument.
fn topic_key(spec: &Spec) -> String {
    spec.path
        .iter()
        .filter_map(|t| match t {
            Tok::K(k) => Some(*k),
            _ => None,
        })
        .skip_while(|k| *k == "no")
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs `debug TOPIC`, `no debug TOPIC` and `show debugging`.
pub(super) fn command(c: &mut Ctx<'_>, spec: &Spec, args: &[String]) -> R {
    match spec.id {
        Id::DebugTopic | Id::NoDebugTopic => {
            let on = spec.id == Id::DebugTopic;
            let key = topic_key(spec);
            let Some((_, kind, what)) = TOPICS.iter().find(|(k, _, _)| *k == key) else {
                return Err("% Nothing to debug there".into());
            };
            if *kind == DebugKind::Packet && on {
                // `debug ip packet ACL`: the list must exist, and filters what is reported.
                let acl = args.first().map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
                if let Some(name) = &acl
                    && !c.d()?.acls.iter().any(|a| &a.name == name)
                {
                    return Err(format!("% There is no access list `{name}`"));
                }
                c.sim.set_debug(c.dev, *kind, true);
                c.sim.set_debug_packet_acl(c.dev, acl);
                return Ok(format!("{} debugging is on: {what}.", kind.name()));
            }
            c.sim.set_debug(c.dev, *kind, on);
            Ok(if on { format!("{} debugging is on: {what}.", kind.name()) } else { format!("{} debugging is off.", kind.name()) })
        }
        Id::UndebugAll => {
            c.sim.clear_debug(c.dev);
            Ok("All debugging is off.".into())
        }
        Id::ShowDebugging => {
            let on = c.sim.debugs(c.dev);
            if on.is_empty() {
                return Ok("No debugging is on.".into());
            }
            let acl = c.sim.state(c.dev).and_then(|s| s.debug_packet_acl.clone());
            let mut line = format!("Debugging is on for: {}.", on.join(", "));
            if let Some(a) = acl {
                line.push_str(&format!("\nIP packet reports are limited to packets list {a} permits."));
            }
            Ok(line)
        }
        _ => Err("% Not a debug command".into()),
    }
}
