//! Debug output for protocols whose state the simulator computes instead of carrying in
//! messages (CDP, spanning-tree roles, HSRP and VRRP states, EIGRP neighbours, routes, the NTP
//! clock), and for frames it only inspects on their way in (BPDUs, LLDP, PPPoE).
//!
//! The computed ones are watched: at the start of each simulation window every device that has
//! one of these topics on gets a snapshot of the facts the topic is about, which is compared with
//! the last snapshot. The first snapshot after the topic is turned on says what is true now; later
//! ones say what appeared, changed or went away.

use std::collections::BTreeMap;

use netlabcraft_core::DeviceId;
use netlabcraft_model::{Device, DeviceKind, Topology, ifname};
use netlabcraft_wire::{Frame, pppoe};

use crate::{DebugKind, Sim};

/// Topics that are watched rather than reported from a frame or an event.
const WATCHED: &[DebugKind] = &[
    DebugKind::CdpEvents,
    DebugKind::CdpPackets,
    DebugKind::StpEvents,
    DebugKind::StpRoot,
    DebugKind::EigrpNeighbors,
    DebugKind::EigrpFsm,
    DebugKind::RipEvents,
    DebugKind::Routing,
    DebugKind::Standby,
    DebugKind::Vrrp,
    DebugKind::Ntp,
];

/// How often a device repeats its CDP advertisements.
const CDP_EVERY_US: u64 = 60_000_000;

/// Most facts one topic keeps for one device, and most messages one snapshot reports.
const MAX_FACTS: usize = 512;
const MAX_REPORT: usize = 64;

/// What a topic is about right now: a name and its value.
type Facts = BTreeMap<String, String>;

fn role_word(r: crate::stp::Role) -> &'static str {
    match r {
        crate::stp::Role::Root => "root port",
        crate::stp::Role::Designated => "designated port",
        crate::stp::Role::Alternate => "alternate port",
        crate::stp::Role::Disabled => "disabled",
    }
}

impl Sim {
    /// Compares what the watched topics are about with the last look, and reports the differences.
    pub(crate) fn debug_watch(&mut self, t: &Topology) {
        let wanted: Vec<(DeviceId, DebugKind)> =
            self.state.iter().flat_map(|(id, s)| WATCHED.iter().filter(move |k| s.debug_on(**k)).map(move |k| (*id, *k))).collect();
        // A topic that was turned off forgets its last look, so turning it on again starts afresh.
        for (id, s) in &mut self.state {
            s.debug_prev.retain(|k, _| wanted.contains(&(*id, *k)));
            s.debug_tick.retain(|k, _| wanted.contains(&(*id, *k)));
        }
        for (id, kind) in wanted {
            let Some(d) = t.device(id) else { continue };
            let now = self.debug_facts(t, d, kind);
            if kind == DebugKind::CdpPackets {
                let due =
                    self.state.get(&id).and_then(|s| s.debug_tick.get(&kind).copied()).is_none_or(|at| self.now.saturating_sub(at) >= CDP_EVERY_US);
                if due {
                    self.st(id).debug_tick.insert(kind, self.now);
                    for (k, v) in now.iter().take(MAX_REPORT) {
                        self.debug_log(id, kind, &format!("advertisement received from {k} ({v})"));
                    }
                }
                continue;
            }
            let prev = self.st(id).debug_prev.remove(&kind);
            let first = prev.is_none();
            let prev = prev.unwrap_or_default();
            let mut said: Vec<String> = Vec::new();
            for (k, v) in &now {
                match prev.get(k) {
                    None if first => said.push(format!("{k}: {v}")),
                    None => said.push(format!("{k} appeared: {v}")),
                    Some(old) if old != v => said.push(format!("{k} changed: {old} -> {v}")),
                    _ => {}
                }
            }
            for (k, old) in &prev {
                if !now.contains_key(k) {
                    said.push(format!("{k} is gone (it was {old})"));
                }
            }
            self.st(id).debug_prev.insert(kind, now);
            for line in said.into_iter().take(MAX_REPORT) {
                self.debug_log(id, kind, &line);
            }
        }
    }

    /// The facts a watched topic is about, for one device.
    fn debug_facts(&mut self, t: &Topology, d: &Device, kind: DebugKind) -> Facts {
        let mut f = Facts::new();
        match kind {
            DebugKind::CdpEvents | DebugKind::CdpPackets => {
                for n in crate::cdp::neighbors(t, d) {
                    f.insert(format!("neighbour {} on {}", n.name, n.local), format!("their port {}, {}", n.remote, n.platform));
                }
            }
            DebugKind::StpEvents | DebugKind::StpRoot if d.kind == DeviceKind::Switch => {
                let vlans: Vec<netlabcraft_core::VlanId> = d.vlans.keys().copied().collect();
                for v in vlans {
                    let tree = self.spanning_tree(t, v).clone();
                    let root = (kind == DebugKind::StpRoot).then(|| tree.bridges.get(&d.id)).flatten();
                    if let Some(b) = root {
                        let here = if b.root == b.id { ", this switch" } else { "" };
                        f.insert(format!("VLAN {v} root"), format!("{}.{} at cost {}{here}", b.root.priority, b.root.mac.dotted(), b.root_cost));
                    }
                    if kind == DebugKind::StpEvents {
                        for ((_, p), info) in tree.ports.iter().filter(|((dev, _), _)| *dev == d.id) {
                            let name = d.ports.get(*p).map(|x| ifname::short(&x.name)).unwrap_or_default();
                            f.insert(format!("VLAN {v} port {name}"), format!("{} ({})", role_word(info.role), info.role.state()));
                        }
                    }
                }
            }
            DebugKind::EigrpNeighbors | DebugKind::EigrpFsm if d.eigrp.is_some() => {
                let st = self.eigrp_state(t);
                if kind == DebugKind::EigrpNeighbors {
                    for n in st.neighbors.get(&d.id).into_iter().flatten() {
                        let v = if n.held_until.is_some() { "held, no hello heard".to_string() } else { format!("up, hold time {} s", n.hold_secs) };
                        f.insert(format!("neighbour {} on {}", n.address, n.iface), v);
                    }
                } else {
                    for r in st.routes.get(&d.id).into_iter().flatten() {
                        f.insert(format!("route {} via {}", r.net, r.via), format!("out {}, metric {}", r.iface, r.metric));
                    }
                }
            }
            DebugKind::RipEvents if d.rip.is_some() => {
                for r in self.rip_routes(t, d) {
                    f.insert(
                        format!("route {} via {}", r.net, r.via),
                        format!("{} hop{} out {}", r.hops, if r.hops == 1 { "" } else { "s" }, r.iface),
                    );
                }
            }
            DebugKind::Routing => {
                for r in self.routing_table(t, d) {
                    let via = r.via.map_or("directly connected".to_string(), |v| format!("via {v}"));
                    f.insert(format!("{} route {} {via}", r.code, r.net), format!("distance {}, metric {}", r.distance, r.metric));
                }
            }
            DebugKind::Standby => {
                let roles = self.hsrp_roles(t);
                for r in roles.get(&d.id).into_iter().flatten() {
                    let a = |x: Option<std::net::Ipv4Addr>| x.map_or("none".to_string(), |x| x.to_string());
                    f.insert(
                        format!("group {} on {}", r.group, r.iface),
                        format!("{}, priority {}, active {}, standby {}", r.state.name(), r.priority, a(r.active), a(r.standby)),
                    );
                }
            }
            DebugKind::Vrrp => {
                let roles = self.fhrp_roles(t);
                for r in roles.vrrp.get(&d.id).into_iter().flatten() {
                    let m = r.master.map_or("none".to_string(), |x| x.to_string());
                    f.insert(format!("group {} on {}", r.group, r.iface), format!("{}, priority {}, master {m}", r.state.name(), r.priority));
                }
            }
            DebugKind::Ntp => {
                let v = match self.state.get(&d.id).and_then(|s| s.ntp_synced) {
                    Some(s) => format!("synchronised with {s}"),
                    None => "not synchronised".to_string(),
                };
                f.insert("clock".to_string(), v);
            }
            _ => {}
        }
        if f.len() > MAX_FACTS {
            f = f.into_iter().take(MAX_FACTS).collect();
        }
        f
    }

    /// A link-local frame (BPDU, LLDP) arrived on a switch or a router: `debug spanning-tree bpdu`
    /// and `debug lldp` describe it.
    pub(crate) fn debug_link_local(&mut self, d: &Device, in_port: usize, f: &Frame) {
        let port = d.ports.get(in_port).map(|p| ifname::short(&p.name)).unwrap_or_default();
        if f.dst == netlabcraft_wire::control::STP_GROUP
            && self.debug_on(d.id, DebugKind::StpBpdu)
            && let Ok(b) = netlabcraft_wire::control::Bpdu::decode(&f.payload)
        {
            let kind = if b.rstp.is_some() { "rapid" } else { "classic" };
            self.debug_log(
                d.id,
                DebugKind::StpBpdu,
                &format!(
                    "{kind} BPDU on {port}: root {}.{} at cost {}, sent by bridge {}.{}",
                    b.root.priority,
                    b.root.mac.dotted(),
                    b.root_cost,
                    b.bridge.priority,
                    b.bridge.mac.dotted()
                ),
            );
        }
        if f.ethertype == netlabcraft_wire::lldp::ETHERTYPE_LLDP
            && self.debug_on(d.id, DebugKind::Lldp)
            && let Ok(l) = netlabcraft_wire::lldp::Lldp::decode(&f.payload)
        {
            self.debug_log(d.id, DebugKind::Lldp, &format!("advertisement on {port} from {} (port {}), kept for {} s", l.system_name, l.port, l.ttl));
        }
    }

    /// A PPPoE frame arrived: discovery and session frames are reported by the topics that care.
    pub(crate) fn debug_pppoe(&mut self, d: &Device, in_port: usize, f: &Frame) {
        let any = [DebugKind::PppoeEvents, DebugKind::PppoePackets, DebugKind::PppNegotiation, DebugKind::PppAuth, DebugKind::PppPacket]
            .iter()
            .any(|k| self.debug_on(d.id, *k));
        if !any {
            return;
        }
        let port = d.ports.get(in_port).map(|p| ifname::short(&p.name)).unwrap_or_default();
        let who = f.src.dotted();
        if f.ethertype == pppoe::ETHERTYPE_DISCOVERY {
            let Ok(m) = pppoe::Discovery::decode(&f.payload) else { return };
            let name = match m.code {
                pppoe::CODE_PADI => "PADI (who offers service?)",
                pppoe::CODE_PADO => "PADO (an offer)",
                pppoe::CODE_PADR => "PADR (a request for a session)",
                pppoe::CODE_PADS => "PADS (the session is confirmed)",
                pppoe::CODE_PADT => "PADT (the session is ended)",
                _ => "an unknown discovery frame",
            };
            self.debug_log(d.id, DebugKind::PppoePackets, &format!("{name} from {who} on {port}, session {}", m.session_id));
            if matches!(m.code, pppoe::CODE_PADR | pppoe::CODE_PADS | pppoe::CODE_PADT) {
                let what = match m.code {
                    pppoe::CODE_PADR => "a client asks for a session",
                    pppoe::CODE_PADS => "a session is confirmed",
                    _ => "a session ends",
                };
                self.debug_log(d.id, DebugKind::PppoeEvents, &format!("{what} ({who} on {port}, session {})", m.session_id));
            }
            return;
        }
        let Ok(sp) = pppoe::SessionPacket::decode(&f.payload) else { return };
        let proto = match sp.protocol {
            pppoe::PPP_LCP => "LCP",
            pppoe::PPP_IPCP => "IPCP",
            pppoe::PPP_PAP => "PAP",
            pppoe::PPP_CHAP => "CHAP",
            pppoe::PPP_IP => "IP data",
            _ => "other",
        };
        let code = pppoe::Control::decode(&sp.data).ok().map(|c| (c.code, c.id));
        let what = match (sp.protocol, code) {
            (pppoe::PPP_PAP, Some((pppoe::PAP_REQUEST, id))) => format!("PAP credentials sent (id {id})"),
            (pppoe::PPP_PAP, Some((pppoe::PAP_ACK, id))) => format!("PAP accepted (id {id})"),
            (pppoe::PPP_PAP, Some((pppoe::PAP_NAK, id))) => format!("PAP refused (id {id})"),
            (pppoe::PPP_CHAP, Some((pppoe::CHAP_CHALLENGE, id))) => format!("CHAP challenge (id {id})"),
            (pppoe::PPP_CHAP, Some((pppoe::CHAP_RESPONSE, id))) => format!("CHAP response (id {id})"),
            (pppoe::PPP_CHAP, Some((pppoe::CHAP_SUCCESS, id))) => format!("CHAP success (id {id})"),
            (pppoe::PPP_CHAP, Some((pppoe::CHAP_FAILURE, id))) => format!("CHAP failure (id {id})"),
            (_, Some((c, id))) => {
                let n = match c {
                    pppoe::CONF_REQ => "Configure-Request",
                    pppoe::CONF_ACK => "Configure-Ack",
                    pppoe::CONF_NAK => "Configure-Nak",
                    pppoe::CONF_REJ => "Configure-Reject",
                    pppoe::TERM_REQ => "Terminate-Request",
                    pppoe::TERM_ACK => "Terminate-Ack",
                    pppoe::ECHO_REQ => "Echo-Request",
                    pppoe::ECHO_REP => "Echo-Reply",
                    _ => "a control packet",
                };
                format!("{n} (id {id})")
            }
            (_, None) => "a data frame".to_string(),
        };
        let line = format!("{proto}: {what} from {who} on {port}, session {}", sp.session_id);
        self.debug_log(d.id, DebugKind::PppPacket, &line);
        match sp.protocol {
            pppoe::PPP_LCP | pppoe::PPP_IPCP if !matches!(code, Some((pppoe::ECHO_REQ | pppoe::ECHO_REP, _))) => {
                self.debug_log(d.id, DebugKind::PppNegotiation, &line);
            }
            pppoe::PPP_PAP | pppoe::PPP_CHAP => self.debug_log(d.id, DebugKind::PppAuth, &line),
            _ => {}
        }
    }
}
