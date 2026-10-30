//! A router's stateful inspection: the zone-based policy firewall and classic inspection
//! (`ip inspect`).
//!
//! Zone firewall: once any interface is a zone member, a routed packet is judged by the zones
//! of the interface it came in on and the one it leaves by. Both outside any zone: it passes,
//! as before. One in a zone and the other not: dropped. Both in the same zone: it passes. Two
//! different zones: the zone pair from the first to the second decides through its inspect
//! policy, class by class (`inspect` lets it through and remembers the session so the replies
//! come back by themselves, `pass` lets it through one way only, `drop` throws it away); a
//! missing pair, a pair without a policy and a packet no class takes are dropped. Traffic to
//! and from the router itself belongs to the `self` zone, which passes unless a pair with
//! `self` says otherwise.
//!
//! Classic inspection: `ip inspect NAME in|out` on an interface remembers the sessions of the
//! protocols the rule names as they cross that interface in that direction; their replies then
//! pass the interfaces' access lists, which stay in force for everything else.
//!
//! Sessions are kept in the router's untranslated (inside) addresses, live in simulated time,
//! and end after the idle timeout of their protocol (shorter once a TCP FIN or RST went by).
//! A TCP session only starts with a SYN. Non-initial fragments carry no ports and are not
//! judged by sessions (they follow their first fragment).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_model::Device;
use netlabcraft_model::security::{CLASS_DEFAULT, InspectAction, InspectClass, InspectMatch, RouterSecurity, SELF_ZONE, protocol_matches};
use netlabcraft_wire::tcp::{self, Tcp};
use netlabcraft_wire::{Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP};

use crate::firewall::{Flow, flow_of, icmp_error, proto_name, quoted_flow};
use crate::{L3If, Sim};

/// Most sessions one router keeps (the oldest go first).
pub const MAX_SESSIONS: usize = 4_096;
/// Idle timeouts (µs).
const TCP_IDLE_US: u64 = 3_600 * 1_000_000;
const UDP_IDLE_US: u64 = 30 * 1_000_000;
const ICMP_IDLE_US: u64 = 10 * 1_000_000;
const CLOSING_US: u64 = 2_000_000;
/// Deepest `match class-map` nesting followed.
const MAX_NESTING: u8 = 8;

/// What a session belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// A zone pair, and the class of its policy that inspected it.
    Pair { pair: String, class: String },
    /// A classic inspection rule, and the interface it is applied to.
    Rule { rule: String, iface: String },
}

/// One inspected session, in the direction it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub owner: Owner,
    pub protocol: u8,
    pub src: Ipv4Addr,
    pub src_port: u16,
    pub dst: Ipv4Addr,
    pub dst_port: u16,
    pub started_us: u64,
    pub last_us: u64,
    /// Packets and bytes, from the initiator and from the responder.
    pub packets: (u64, u64),
    pub bytes: (u64, u64),
    pub closing: bool,
}

impl Session {
    fn forward(&self, f: &Flow) -> bool {
        self.protocol == f.protocol && self.src == f.src && self.src_port == f.src_port && self.dst == f.dst && self.dst_port == f.dst_port
    }

    fn reverse(&self, f: &Flow) -> bool {
        self.protocol == f.protocol && self.src == f.dst && self.src_port == f.dst_port && self.dst == f.src && self.dst_port == f.src_port
    }

    fn idle_limit_us(&self) -> u64 {
        if self.closing {
            return CLOSING_US;
        }
        match self.protocol {
            PROTO_TCP => TCP_IDLE_US,
            PROTO_ICMP => ICMP_IDLE_US,
            _ => UDP_IDLE_US,
        }
    }

    pub fn protocol_name(&self) -> &'static str {
        proto_name(self.protocol)
    }
}

/// A router's sessions and counters.
#[derive(Debug, Clone, Default)]
pub struct ZfwState {
    pub sessions: Vec<Session>,
    /// Packets each (zone pair, class) took, with the action's word.
    pub class_hits: BTreeMap<(String, String), u64>,
    /// Packets dropped, by reason.
    pub drops: BTreeMap<String, u64>,
}

/// Whether a packet is a fragment other than the first (it carries no transport header).
fn later_fragment(raw: &[u8]) -> bool {
    raw.get(6..8).is_some_and(|b| u16::from_be_bytes([b[0], b[1]]) & 0x1fff != 0)
}

fn tcp_flags(ip: &Ipv4) -> Option<u8> {
    (ip.protocol == PROTO_TCP).then(|| Tcp::decode(&ip.payload, ip.src, ip.dst).ok().map(|s| s.flags)).flatten()
}

/// Whether one class map takes a packet.
fn class_matches(d: &Device, sec: &RouterSecurity, class: &InspectClass, ip: &Ipv4, flow: &Flow, depth: u8) -> bool {
    if class.matches.is_empty() || depth > MAX_NESTING {
        return false;
    }
    let dst_port = matches!(ip.protocol, PROTO_TCP | PROTO_UDP).then_some(flow.dst_port);
    let one = |m: &InspectMatch| match m {
        InspectMatch::Protocol(p) => protocol_matches(p, ip.protocol, dst_port),
        InspectMatch::AccessGroup(acl) => crate::vpn::acl_permits(d, acl, ip),
        InspectMatch::ClassMap(name) => sec.class(name).is_some_and(|c| class_matches(d, sec, c, ip, flow, depth.saturating_add(1))),
    };
    if class.match_all { class.matches.iter().all(one) } else { class.matches.iter().any(one) }
}

/// Whether a classic inspection rule names the packet's protocol.
fn rule_matches(sec: &RouterSecurity, rule: &str, ip: &Ipv4, flow: &Flow) -> bool {
    let dst_port = matches!(ip.protocol, PROTO_TCP | PROTO_UDP).then_some(flow.dst_port);
    sec.rule(rule).is_some_and(|r| r.protocols.iter().any(|p| protocol_matches(p, ip.protocol, dst_port)))
}

fn count(map: &mut BTreeMap<String, u64>, key: String) {
    let n = map.entry(key).or_insert(0);
    *n = n.saturating_add(1);
}

impl Sim {
    /// Forgets idle sessions.
    fn zfw_expire(&mut self, d: &Device) {
        let now = self.now;
        self.st(d.id).zfw.sessions.retain(|s| now.saturating_sub(s.last_us) <= s.idle_limit_us());
    }

    /// A packet of a known session (either way), or an ICMP error about one: counted, and true.
    fn zfw_known(&mut self, d: &Device, ip: &Ipv4, raw: &[u8], rules_only: bool) -> bool {
        if later_fragment(raw) {
            return false;
        }
        self.zfw_expire(d);
        let flow = flow_of(ip);
        let quoted = icmp_error(ip).and_then(|(_, _, original)| quoted_flow(&original));
        let flags = tcp_flags(ip);
        let now = self.now;
        let len = u64::try_from(raw.len()).unwrap_or(u64::MAX);
        let st = self.st(d.id);
        for s in st.zfw.sessions.iter_mut().filter(|s| !rules_only || matches!(s.owner, Owner::Rule { .. })) {
            let (fwd, rev) = (s.forward(&flow), s.reverse(&flow));
            // An ICMP error about the session's packets, from either side or a router between.
            let about = quoted.is_some_and(|q| s.forward(&q) || s.reverse(&q));
            if !(fwd || rev || about) {
                continue;
            }
            s.last_us = now;
            if fwd {
                s.packets.0 = s.packets.0.saturating_add(1);
                s.bytes.0 = s.bytes.0.saturating_add(len);
            } else {
                s.packets.1 = s.packets.1.saturating_add(1);
                s.bytes.1 = s.bytes.1.saturating_add(len);
            }
            if flags.is_some_and(|f| f & (tcp::FIN | tcp::RST) != 0) {
                s.closing = true;
            }
            return true;
        }
        false
    }

    fn zfw_open(&mut self, d: &Device, owner: Owner, ip: &Ipv4, raw: &[u8]) {
        let flow = flow_of(ip);
        let now = self.now;
        let len = u64::try_from(raw.len()).unwrap_or(u64::MAX);
        let st = self.st(d.id);
        if st.zfw.sessions.len() >= MAX_SESSIONS {
            st.zfw.sessions.remove(0);
        }
        st.zfw.sessions.push(Session {
            owner,
            protocol: flow.protocol,
            src: flow.src,
            src_port: flow.src_port,
            dst: flow.dst,
            dst_port: flow.dst_port,
            started_us: now,
            last_us: now,
            packets: (1, 0),
            bytes: (len, 0),
            closing: false,
        });
    }

    fn zfw_drop(&mut self, d: &Device, why: String, log: bool) -> Result<(), String> {
        count(&mut self.st(d.id).zfw.drops, why.clone());
        self.bump(d.id, "dropped by the zone firewall");
        if log {
            self.log(d.id, &format!("Zone firewall: {why}"));
        }
        Err(format!("zone firewall: {why}"))
    }

    /// The zone firewall's verdict on a packet going from `from` to `to` (`None`: the router
    /// itself). `raw` is the datagram as it arrived (or will leave).
    pub(crate) fn zfw_check(&mut self, d: &Device, from: Option<&L3If>, to: Option<&L3If>, ip: &Ipv4, raw: &[u8]) -> Result<(), String> {
        let sec = &d.security;
        if !sec.zones_active() || d.kind != netlabcraft_model::DeviceKind::Router {
            return Ok(());
        }
        let zone = |i: Option<&L3If>| match i {
            Some(i) => sec.zone_of(&i.name).map(str::to_string),
            None => Some(SELF_ZONE.to_string()),
        };
        let (zin, zout) = (zone(from), zone(to));
        let name = |i: Option<&L3If>| i.map_or_else(|| "the router".to_string(), |i| i.name.clone());
        let (src, dst) = match (zin, zout) {
            (None, None) => return Ok(()),
            (Some(z), None) | (None, Some(z)) if z == SELF_ZONE => return Ok(()),
            (Some(z), None) => return self.zfw_drop(d, format!("{} is in zone {z} but {} is in no zone", name(from), name(to)), false),
            (None, Some(z)) => return self.zfw_drop(d, format!("{} is in no zone but {} is in zone {z}", name(from), name(to)), false),
            (Some(a), Some(b)) if a == b => return Ok(()),
            (Some(a), Some(b)) => (a, b),
        };
        if self.zfw_known(d, ip, raw, false) || later_fragment(raw) {
            return Ok(());
        }
        let Some(pair) = sec.pair_between(&src, &dst) else {
            if src == SELF_ZONE || dst == SELF_ZONE {
                return Ok(());
            }
            return self.zfw_drop(d, format!("no zone pair from {src} to {dst}"), false);
        };
        let Some(policy) = pair.policy.as_deref().and_then(|p| sec.policy(p)) else {
            return self.zfw_drop(d, format!("zone pair {} has no inspect policy", pair.name), false);
        };
        let flow = flow_of(ip);
        let taken = policy
            .classes
            .iter()
            .find(|c| c.class.eq_ignore_ascii_case(CLASS_DEFAULT) || sec.class(&c.class).is_some_and(|cl| class_matches(d, sec, cl, ip, &flow, 0)));
        let (class, action, log) = match taken {
            Some(c) => (c.class.clone(), c.effective(), c.log),
            None => (CLASS_DEFAULT.to_string(), InspectAction::Drop, false),
        };
        let hits = self.st(d.id).zfw.class_hits.entry((pair.name.clone(), class.clone())).or_insert(0);
        *hits = hits.saturating_add(1);
        let what = format!("{} {}:{} → {}:{}", proto_name(ip.protocol), ip.src, flow.src_port, ip.dst, flow.dst_port);
        match action {
            InspectAction::Drop => self.zfw_drop(d, format!("{what} dropped by class {class} of zone pair {}", pair.name), log),
            InspectAction::Pass => {
                if log {
                    self.log(d.id, &format!("Zone firewall: {what} passed by class {class} of zone pair {}", pair.name));
                }
                Ok(())
            }
            InspectAction::Inspect => {
                // A TCP session starts with a SYN; anything else has no session to belong to.
                if tcp_flags(ip).is_some_and(|f| f & (tcp::SYN | tcp::ACK) != tcp::SYN) {
                    return self.zfw_drop(d, format!("{what} is not a SYN and belongs to no session (zone pair {})", pair.name), false);
                }
                self.zfw_open(d, Owner::Pair { pair: pair.name.clone(), class }, ip, raw);
                Ok(())
            }
        }
    }

    /// Classic inspection: whether a packet arriving on a router is the reply of an inspected
    /// session (it then passes the access lists).
    pub(crate) fn cbac_return(&mut self, d: &Device, ip: &Ipv4, raw: &[u8]) -> bool {
        if d.security.inspect_applied.is_empty() || d.kind != netlabcraft_model::DeviceKind::Router {
            return false;
        }
        self.zfw_known(d, ip, raw, true)
    }

    /// Classic inspection: opens a session for a packet that crossed `ifc` in the direction an
    /// applied rule watches.
    pub(crate) fn cbac_open(&mut self, d: &Device, in_if: &L3If, out_if: &L3If, ip: &Ipv4, raw: &[u8]) {
        let sec = &d.security;
        if sec.inspect_applied.is_empty() || d.kind != netlabcraft_model::DeviceKind::Router || later_fragment(raw) {
            return;
        }
        let flow = flow_of(ip);
        let watched = sec
            .inspect_applied
            .get(&in_if.name)
            .and_then(|b| b.inbound.clone())
            .filter(|r| rule_matches(sec, r, ip, &flow))
            .map(|r| (r, in_if.name.clone()))
            .or_else(|| {
                sec.inspect_applied
                    .get(&out_if.name)
                    .and_then(|b| b.outbound.clone())
                    .filter(|r| rule_matches(sec, r, ip, &flow))
                    .map(|r| (r, out_if.name.clone()))
            });
        let Some((rule, iface)) = watched else { return };
        if tcp_flags(ip).is_some_and(|f| f & (tcp::SYN | tcp::ACK) != tcp::SYN) {
            return;
        }
        self.zfw_open(d, Owner::Rule { rule, iface }, ip, raw);
    }

    /// The router's inspected sessions and counters.
    pub fn zfw_state(&mut self, d: &Device) -> ZfwState {
        self.zfw_expire(d);
        self.state.get(&d.id).map(|s| s.zfw.clone()).unwrap_or_default()
    }

    /// Forgets the sessions (`clear zone-pair inspect sessions`, `clear ip inspect sessions`).
    pub fn zfw_clear(&mut self, d: netlabcraft_core::DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.zfw = ZfwState::default();
        }
    }
}
