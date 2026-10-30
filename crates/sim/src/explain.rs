//! Why a device did what it did with a frame: the small, structured decisions the simulator
//! records for each event (MAC table lookups, routing lookups, access lists, NAT, ARP), and the
//! OSI-layer view built from them for Simulation mode's PDU details.
//!
//! Recording is cheap and bounded: a few short steps per event ([`MAX_STEPS`]), kept only while
//! the receiving device handles the frame.

use std::net::Ipv4Addr;

use netlabcraft_core::{MacAddr, VlanId};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::tcp::Tcp;
use netlabcraft_wire::{Arp, ArpOp, ETHERTYPE_ARP, ETHERTYPE_IPV4, Frame, Icmp, Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP, Udp};

use crate::{L3If, Sim, TraceEvent};

/// Most decisions kept per event.
pub const MAX_STEPS: usize = 8;

/// One decision a device took about a frame it received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The frame's destination MAC is the receiving interface's own.
    ForMe { iface: String },
    /// A broadcast (or group) frame, taken by the interface.
    Broadcast { iface: String },
    /// A switch found the destination MAC in its table.
    Switched { vlan: u16, mac: MacAddr, out: String },
    /// A switch didn't know the destination (or it is a broadcast) and flooded the VLAN.
    Flooded { vlan: u16, ports: u16, broadcast: bool },
    /// An access list let the packet through.
    AclPermit { acl: String, iface: String, inbound: bool },
    /// NAT, inside to outside: the source address rewritten.
    NatOut { from: Ipv4Addr, to: Ipv4Addr },
    /// NAT, outside to inside: the destination address restored.
    NatIn { from: Ipv4Addr, to: Ipv4Addr },
    /// The routing table's answer for the destination.
    Routed { dst: Ipv4Addr, out: String, via: Option<Ipv4Addr>, ttl: u8 },
    /// The next hop's MAC came from the ARP table.
    ArpKnown { ip: Ipv4Addr, mac: MacAddr },
    /// The next hop's MAC is unknown: an ARP request goes out first.
    ArpAsk { ip: Ipv4Addr, iface: String },
    /// The packet is for this device.
    Delivered { dst: Ipv4Addr },
}

impl Step {
    /// The OSI layer the decision belongs to, and whether it is about the frame coming in
    /// (true) or going out.
    pub fn place(&self) -> (u8, bool) {
        match self {
            Step::ForMe { .. } | Step::Broadcast { .. } => (2, true),
            Step::Switched { .. } | Step::Flooded { .. } | Step::ArpKnown { .. } | Step::ArpAsk { .. } => (2, false),
            Step::AclPermit { inbound, .. } => (3, *inbound),
            Step::NatIn { .. } | Step::Routed { .. } | Step::Delivered { .. } => (3, true),
            Step::NatOut { .. } => (3, false),
        }
    }

    /// The decision in a sentence (each sentence is a fixed template, so it can be translated).
    pub fn describe(&self) -> String {
        match self {
            Step::ForMe { iface } => fill("The frame's destination MAC is the MAC of {}, so the device takes it.", &[iface]),
            Step::Broadcast { iface } => fill("The frame is a broadcast, so {} takes it.", &[iface]),
            Step::Switched { vlan, mac, out } => {
                fill("The MAC table has {} in VLAN {} on {}: the frame goes out that port only.", &[&mac.to_string(), &vlan.to_string(), out])
            }
            Step::Flooded { vlan, ports, broadcast: true } => {
                fill("A broadcast: the switch floods it out the other {} ports of VLAN {}.", &[&ports.to_string(), &vlan.to_string()])
            }
            Step::Flooded { vlan, ports, .. } => fill(
                "The destination MAC is not in the MAC table: the switch floods the frame out the other {} ports of VLAN {}.",
                &[&ports.to_string(), &vlan.to_string()],
            ),
            Step::AclPermit { acl, iface, inbound: true } => fill("Access list {} on {} (in) permits the packet.", &[acl, iface]),
            Step::AclPermit { acl, iface, .. } => fill("Access list {} on {} (out) permits the packet.", &[acl, iface]),
            Step::NatOut { from, to } => fill("NAT translates the inside source {} to {}.", &[&from.to_string(), &to.to_string()]),
            Step::NatIn { from, to } => {
                fill("NAT translates the destination {} back to the inside address {}.", &[&from.to_string(), &to.to_string()])
            }
            Step::Routed { dst, out, via: Some(nh), ttl } => fill(
                "The device looks up {} in its routing table: next hop {}, out {}. TTL drops to {}.",
                &[&dst.to_string(), &nh.to_string(), out, &ttl.to_string()],
            ),
            Step::Routed { dst, out, via: None, ttl } => fill(
                "The device looks up {} in its routing table: directly connected, out {}. TTL drops to {}.",
                &[&dst.to_string(), out, &ttl.to_string()],
            ),
            Step::ArpKnown { ip, mac } => fill("The ARP table gives {} for {}: the frame is addressed to it.", &[&mac.to_string(), &ip.to_string()]),
            Step::ArpAsk { ip, iface } => fill("{} is not in the ARP table: an ARP request for it goes out {} first.", &[&ip.to_string(), iface]),
            Step::Delivered { dst } => fill("{} is this device's own address: the packet is for it.", &[&dst.to_string()]),
        }
    }
}

/// A template with its `{}` filled in order.
fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    let mut it = args.iter();
    while let Some(at) = rest.find("{}") {
        out.push_str(rest.get(..at).unwrap_or_default());
        out.push_str(it.next().copied().unwrap_or_default());
        rest = rest.get(at + 2..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

impl Sim {
    /// Records a decision about the frame being received (nothing outside a delivery).
    pub(crate) fn note(&mut self, step: Step) {
        if let Some(v) = &mut self.explaining
            && v.len() < MAX_STEPS
        {
            v.push(step);
        }
    }

    /// Whether decisions are being recorded now (to skip work that only feeds them).
    pub(crate) fn explaining(&self) -> bool {
        self.explaining.is_some()
    }

    /// The access list an interface applies (in or out), for the record.
    pub(crate) fn note_acl(&mut self, d: &Device, ifc: &L3If, inbound: bool) {
        if !self.explaining() {
            return;
        }
        let groups = if ifc.port == crate::SVI_PORT {
            d.svis.iter().find(|s| Some(s.vlan.get()) == ifc.vlan).map(|s| &s.access)
        } else {
            d.ports.get(ifc.port).and_then(|p| match ifc.sub {
                Some(n) => p.subif(n).map(|s| &s.access),
                None => Some(&p.access),
            })
        };
        if let Some(acl) = groups.and_then(|g| if inbound { g.inbound.clone() } else { g.outbound.clone() }) {
            self.note(Step::AclPermit { acl, iface: ifc.name.clone(), inbound });
        }
    }

    /// A switch's forwarding decision.
    pub(crate) fn note_switch(&mut self, d: &Device, vlan: VlanId, known: Option<usize>, outs: &[usize], dst: MacAddr) {
        if !self.explaining() {
            return;
        }
        match known {
            Some(p) => {
                let out = d.ports.get(p).map_or_else(|| p.to_string(), |x| x.name.clone());
                self.note(Step::Switched { vlan: vlan.get(), mac: dst, out });
            }
            None => self.note(Step::Flooded { vlan: vlan.get(), ports: u16::try_from(outs.len()).unwrap_or(u16::MAX), broadcast: dst.is_group() }),
        }
    }

    /// The events a device sent because of event `id` (what it did with that frame).
    pub fn caused_by(&self, id: u64) -> Vec<&TraceEvent> {
        self.trace.iter().filter(|e| e.cause == Some(id)).take(64).collect()
    }
}

/// One side of the OSI view: what each layer (index 0 = layer 1) says.
pub type Layers = [Vec<String>; 7];

/// The OSI view of one event at its receiving device.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Osi {
    pub inbound: Layers,
    pub outbound: Layers,
    /// The events the device sent in answer (their ids).
    pub sent: Vec<u64>,
    /// Why the device dropped the frame, and the layer (index) that dropped it.
    pub dropped: Option<(usize, String)>,
}

fn port_name(t: &Topology, e: netlabcraft_model::End) -> String {
    t.device(e.device).and_then(|d| d.ports.get(e.port)).map_or_else(|| "?".to_string(), |p| p.name.clone())
}

/// What a layer-4 header says, and the application it carries.
fn upper_layers(ip: &Ipv4, out: &mut Layers) {
    let app = |port: u16| -> Option<&'static str> {
        Some(match port {
            20 | 21 => "FTP",
            22 => "SSH",
            23 => "Telnet",
            25 => "SMTP",
            53 => "DNS",
            67 | 68 => "DHCP",
            69 => "TFTP",
            80 => "HTTP",
            110 => "POP3",
            123 => "NTP",
            161 | 162 => "SNMP",
            514 => "Syslog",
            _ => return None,
        })
    };
    match ip.protocol {
        PROTO_ICMP => {
            let what = match Icmp::decode(&ip.payload) {
                Ok(Icmp::EchoRequest { seq, .. }) => fill("ICMP echo request, sequence number {}.", &[&seq.to_string()]),
                Ok(Icmp::EchoReply { seq, .. }) => fill("ICMP echo reply, sequence number {}.", &[&seq.to_string()]),
                Ok(Icmp::Unreachable { code, .. }) => fill("ICMP destination unreachable (code {}).", &[&code.to_string()]),
                Ok(Icmp::TimeExceeded { .. }) => "ICMP time exceeded: a TTL ran out.".to_string(),
                Err(_) => "An ICMP message.".to_string(),
            };
            if let Some(l) = out.get_mut(2) {
                l.push(what);
            }
        }
        PROTO_UDP => {
            if let Ok(u) = Udp::decode(&ip.payload, ip.src, ip.dst) {
                if let Some(l) = out.get_mut(3) {
                    l.push(fill(
                        "UDP from port {} to port {}, {} bytes of data.",
                        &[&u.src_port.to_string(), &u.dst_port.to_string(), &u.payload.len().to_string()],
                    ));
                }
                if let Some(a) = app(u.dst_port).or(app(u.src_port))
                    && let Some(l) = out.get_mut(6)
                {
                    l.push(fill("Application: {} message.", &[a]));
                }
            }
        }
        PROTO_TCP => {
            if let Ok(s) = Tcp::decode(&ip.payload, ip.src, ip.dst) {
                let flags = s.flag_names();
                if let Some(l) = out.get_mut(3) {
                    l.push(fill(
                        "TCP from port {} to port {}, flags {}, sequence number {}.",
                        &[&s.src_port.to_string(), &s.dst_port.to_string(), &flags, &s.seq.to_string()],
                    ));
                }
                if let Some(a) = app(s.dst_port).or(app(s.src_port))
                    && let Some(l) = out.get_mut(6)
                {
                    l.push(if s.payload.is_empty() {
                        fill("Application: {} connection (no data in this segment).", &[a])
                    } else {
                        fill("Application: {} data.", &[a])
                    });
                }
            }
        }
        other => {
            if let Some(l) = out.get_mut(3) {
                l.push(fill("IP protocol {}.", &[&other.to_string()]));
            }
        }
    }
}

/// The layers a frame shows, as received at (`inbound`) or sent from `port`.
fn frame_layers(frame: &[u8], port: &str, inbound: bool, out: &mut Layers) {
    if let Some(l) = out.get_mut(0) {
        l.push(if inbound { fill("The frame arrives on {}.", &[port]) } else { fill("The frame leaves on {}.", &[port]) });
    }
    let Ok(f) = Frame::decode(frame) else { return };
    if let Some(l) = out.get_mut(1) {
        l.push(fill("Ethernet frame from {} to {}.", &[&f.src.to_string(), &f.dst.to_string()]));
        if let Some(v) = f.vlan {
            l.push(fill("802.1Q tag: VLAN {}.", &[&v.to_string()]));
        }
    }
    match f.ethertype {
        ETHERTYPE_ARP => {
            if let (Ok(a), Some(l)) = (Arp::decode(&f.payload), out.get_mut(1)) {
                l.push(match a.op {
                    ArpOp::Request => fill("ARP request: who has {}? (asked by {})", &[&a.target_ip.to_string(), &a.sender_ip.to_string()]),
                    ArpOp::Reply => fill("ARP reply: {} is at {}.", &[&a.sender_ip.to_string(), &a.sender_mac.to_string()]),
                });
            }
        }
        ETHERTYPE_IPV4 => {
            if let Ok(ip) = Ipv4::decode(&f.payload) {
                if let Some(l) = out.get_mut(2) {
                    l.push(fill("IP packet from {} to {}, TTL {}.", &[&ip.src.to_string(), &ip.dst.to_string(), &ip.ttl.to_string()]));
                }
                upper_layers(&ip, out);
            }
        }
        _ => {}
    }
}

/// The layer a drop reason belongs to.
fn drop_layer(why: &str) -> usize {
    let w = why.to_ascii_lowercase();
    if w.contains("shut down") || w.contains("switched off") || w.contains("framing") || w.contains("cable") {
        0
    } else if w.contains("vlan")
        || w.contains("spanning tree")
        || w.contains("mac")
        || w.contains("addressed to this device")
        || w.contains("port security")
        || w.contains("switching loop")
        || w.contains("trunk")
        || w.contains("arrival port")
        || w.contains("etherchannel")
    {
        1
    } else if w.contains("tcp") || w.contains("udp port") {
        3
    } else {
        2
    }
}

/// The OSI view of event `e` at the device that received it.
pub fn osi(t: &Topology, sim: &Sim, e: &TraceEvent) -> Osi {
    let mut v = Osi::default();
    frame_layers(&e.frame, &port_name(t, e.to), true, &mut v.inbound);
    v.dropped = e.dropped.as_ref().map(|why| (drop_layer(why), why.clone()));
    let sent = sim.caused_by(e.id);
    v.sent = sent.iter().map(|x| x.id).collect();
    if let Some(first) = sent.first() {
        frame_layers(&first.frame, &port_name(t, first.from), false, &mut v.outbound);
        if sent.len() > 1
            && let Some(l) = v.outbound.get_mut(0)
        {
            let ports: Vec<String> = sent.iter().map(|x| port_name(t, x.from)).collect();
            l.push(fill("Copies leave on {} ports: {}.", &[&sent.len().to_string(), &ports.join(", ")]));
        }
    }
    // The decisions, after what each layer saw.
    for s in &e.explain {
        let (layer, inbound) = s.place();
        let side = if inbound { &mut v.inbound } else { &mut v.outbound };
        if let Some(l) = side.get_mut(usize::from(layer).saturating_sub(1)) {
            l.push(s.describe());
        }
    }
    v
}

/// The OSI view as plain lines (the engine's `sim.event`), English.
pub fn osi_lines(v: &Osi) -> (Layers, Layers) {
    let mut inbound = v.inbound.clone();
    if let Some((layer, why)) = &v.dropped
        && let Some(l) = inbound.get_mut(*layer)
    {
        l.push(fill("The device drops the frame: {}.", &[why]));
    }
    (inbound, v.outbound.clone())
}

/// The layers' names, layer 1 first.
pub const LAYER_NAMES: [&str; 7] = [
    "Layer 1: Physical",
    "Layer 2: Data Link",
    "Layer 3: Network",
    "Layer 4: Transport",
    "Layer 5: Session",
    "Layer 6: Presentation",
    "Layer 7: Application",
];
