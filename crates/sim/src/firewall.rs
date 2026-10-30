//! The firewall appliance's data path: every routed packet is matched against the connection
//! table; a packet that starts a connection must be allowed by the security levels of the two
//! interfaces or by the access list bound to the interface it arrived on (and, when one is
//! bound, by the list on the interface it leaves by). Replies of a tracked connection always
//! pass. Twice NAT, then object NAT, translate on the way through and are undone for replies.
//!
//! ICMP echo is tracked only when the global policy inspects ICMP; otherwise every echo and
//! echo reply is checked like a new packet. An ICMP error about a tracked connection passes,
//! with the translation undone in the packet it quotes too, so traceroute works through NAT.
//! The firewall does not count as a hop (TTL is left alone) unless the global policy says
//! `set connection decrement-ttl`.
//!
//! Connections and translations live in simulated time: an idle connection is forgotten after
//! the firewall's timeout for its protocol, a dynamic translation once no connection uses it
//! and it has been idle for [`XLATE_IDLE_US`].

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_model::Device;
use netlabcraft_model::Topology;
use netlabcraft_model::firewall::{FlowKey, NatKind, ObjAddr};
use netlabcraft_wire::tcp::{self, Tcp};
use netlabcraft_wire::{Icmp, Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP, Udp};

use crate::{L3If, Sim, flow_id, l3_ifaces, rewrite};

/// Most connections and translations one firewall tracks (the oldest go first).
pub const MAX_CONNS: usize = 4_096;
pub const MAX_XLATES: usize = 4_096;
/// A dynamic translation no connection uses is forgotten after this long idle (µs).
pub const XLATE_IDLE_US: u64 = 30_000_000;
/// A TCP connection that saw FIN or RST is forgotten after this long idle (µs).
const CLOSING_US: u64 = 2_000_000;

/// A connection the firewall let through, in real (untranslated) addresses. Ports are the
/// TCP/UDP ports, or the ICMP echo identifier on both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conn {
    pub protocol: u8,
    pub src: Ipv4Addr,
    pub src_port: u16,
    pub dst: Ipv4Addr,
    pub dst_port: u16,
    /// The nameifs it came in on and leaves by.
    pub from_if: String,
    pub to_if: String,
    pub started_us: u64,
    pub last_us: u64,
    pub packets: u64,
    pub bytes: u64,
    /// TCP: a FIN or RST went by.
    pub closing: bool,
}

impl Conn {
    fn forward(&self, k: &Flow) -> bool {
        self.protocol == k.protocol && self.src == k.src && self.src_port == k.src_port && self.dst == k.dst && self.dst_port == k.dst_port
    }

    fn reverse(&self, k: &Flow) -> bool {
        self.protocol == k.protocol && self.src == k.dst && self.src_port == k.dst_port && self.dst == k.src && self.dst_port == k.src_port
    }

    /// How long it may stay idle (µs).
    pub fn idle_limit_us(&self, t: &netlabcraft_model::firewall::Timeouts) -> u64 {
        if self.closing {
            return CLOSING_US;
        }
        let secs = match self.protocol {
            PROTO_ICMP => t.icmp,
            PROTO_UDP => t.udp,
            _ => t.conn,
        };
        u64::from(secs).saturating_mul(1_000_000)
    }

    /// The protocol's name for tables.
    pub fn protocol_name(&self) -> &'static str {
        proto_name(self.protocol)
    }
}

/// An address (and port) translation in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xlate {
    pub protocol: u8,
    pub real: Ipv4Addr,
    pub real_port: u16,
    pub mapped: Ipv4Addr,
    pub mapped_port: u16,
    pub real_if: String,
    pub mapped_if: String,
    /// The rule that made it: a network object's name, or `twice N`.
    pub object: String,
    pub last_us: u64,
}

/// A firewall's dynamic state.
#[derive(Debug, Clone, Default)]
pub struct FwState {
    pub conns: Vec<Conn>,
    pub xlates: Vec<Xlate>,
    /// Translations made per rule (a network object's name, or `twice N`): on the way out…
    pub nat_hits: BTreeMap<String, u64>,
    /// …and undone on the way back.
    pub nat_unhits: BTreeMap<String, u64>,
    /// Packets dropped by policy.
    pub denied: u64,
    /// ICMP packets the global policy's `inspect icmp` tracked.
    pub inspected_icmp: u64,
    /// ICMP errors let through because they belong to a connection.
    pub icmp_errors: u64,
    /// `icmp` rule hits, per interface and rule index (`usize::MAX`: no rule matched).
    pub icmp_hits: BTreeMap<(String, usize), u64>,
}

/// A packet's flow: protocol, addresses and ports (the echo id for ICMP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flow {
    protocol: u8,
    src: Ipv4Addr,
    src_port: u16,
    dst: Ipv4Addr,
    dst_port: u16,
}

fn flow_of(ip: &Ipv4) -> Flow {
    let (src_port, dst_port) = match ip.protocol {
        PROTO_TCP => Tcp::decode(&ip.payload, ip.src, ip.dst).map_or((0, 0), |s| (s.src_port, s.dst_port)),
        PROTO_UDP => Udp::decode(&ip.payload, ip.src, ip.dst).map_or((0, 0), |u| (u.src_port, u.dst_port)),
        PROTO_ICMP => match Icmp::decode(&ip.payload) {
            Ok(Icmp::EchoRequest { id, .. } | Icmp::EchoReply { id, .. }) => (id, id),
            _ => (0, 0),
        },
        _ => (0, 0),
    };
    Flow { protocol: ip.protocol, src: ip.src, src_port, dst: ip.dst, dst_port }
}

/// The ICMP message type of a packet (`None`: not ICMP, or unreadable).
fn icmp_type(ip: &Ipv4) -> Option<u8> {
    (ip.protocol == PROTO_ICMP).then(|| ip.payload.first().copied()).flatten()
}

/// An ICMP error's parts: whether it is a time-exceeded, its code and the quoted packet.
fn icmp_error(ip: &Ipv4) -> Option<(bool, u8, Vec<u8>)> {
    if ip.protocol != PROTO_ICMP {
        return None;
    }
    match Icmp::decode(&ip.payload).ok()? {
        Icmp::TimeExceeded { original } => Some((true, 0, original)),
        Icmp::Unreachable { code, original } => Some((false, code, original)),
        _ => None,
    }
}

/// Where the flow id sits in a quoted packet: the echo id, or the source port.
fn quoted_id_at(original: &[u8]) -> Option<(usize, usize, u8)> {
    let ihl = usize::from(original.first()? & 0x0f) * 4;
    let proto = *original.get(9)?;
    Some((ihl, if proto == PROTO_ICMP { ihl + 4 } else { ihl }, proto))
}

fn addr_at(b: &[u8], at: usize) -> Option<Ipv4Addr> {
    Some(Ipv4Addr::new(*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?))
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

/// The flow of the packet an ICMP error quotes.
fn quoted_flow(original: &[u8]) -> Option<Flow> {
    let (ihl, _, proto) = quoted_id_at(original)?;
    let src = addr_at(original, 12)?;
    let dst = addr_at(original, 16)?;
    let (src_port, dst_port) = match proto {
        PROTO_TCP | PROTO_UDP => (u16_at(original, ihl)?, u16_at(original, ihl + 2)?),
        PROTO_ICMP if matches!(original.get(ihl), Some(0 | 8)) => {
            let id = u16_at(original, ihl + 4)?;
            (id, id)
        }
        _ => (0, 0),
    };
    Some(Flow { protocol: proto, src, src_port, dst, dst_port })
}

/// The quoted packet with an address (at byte 12: source, 16: destination) and maybe its flow
/// id replaced, its header checksum made right again.
fn requote(mut original: Vec<u8>, at: usize, addr: Ipv4Addr, id: Option<(usize, u16)>) -> Option<Vec<u8>> {
    original.get_mut(at..at + 4)?.copy_from_slice(&addr.octets());
    if let Some((id_at, v)) = id {
        original.get_mut(id_at..id_at + 2)?.copy_from_slice(&v.to_be_bytes());
    }
    let ihl = usize::from(original.first()? & 0x0f) * 4;
    original.get_mut(10..12)?.copy_from_slice(&[0, 0]);
    let sum = netlabcraft_wire::checksum(&[original.get(..ihl)?]);
    original.get_mut(10..12)?.copy_from_slice(&sum.to_be_bytes());
    Some(original)
}

fn error_with(te: bool, code: u8, original: Vec<u8>) -> Vec<u8> {
    if te { Icmp::TimeExceeded { original }.encode() } else { Icmp::Unreachable { code, original }.encode() }
}

/// The protocol's name for tables and messages.
pub fn proto_name(p: u8) -> &'static str {
    match p {
        PROTO_ICMP => "icmp",
        PROTO_TCP => "tcp",
        PROTO_UDP => "udp",
        _ => "ip",
    }
}

/// Whether the protocol has ports (or an echo id) NAT can translate.
fn has_ports(p: u8) -> bool {
    matches!(p, PROTO_ICMP | PROTO_TCP | PROTO_UDP)
}

/// The label twice NAT rule `i` (from 0) goes by in counters and translations.
pub fn twice_label(i: usize) -> String {
    format!("twice {}", i + 1)
}

fn bump(map: &mut BTreeMap<String, u64>, key: String) {
    let n = map.entry(key).or_insert(0);
    *n = n.saturating_add(1);
}

impl Sim {
    /// A routed packet arriving at the firewall: translations undone, the connection table and
    /// the policy consulted, the source translated, and the packet sent on.
    pub(crate) fn fw_ip_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: Ipv4, payload: &[u8]) -> Result<(), String> {
        let conf = &d.firewall;
        let Some((in_name, in_level)) = conf.zone(&ifc.name) else {
            return Err(format!("{} has no nameif: the firewall passes nothing on it", ifc.name));
        };
        let now = self.now;
        self.fw_expire(d, now);
        let is_error = icmp_error(&ip).is_some();
        let (ip, src_back) =
            if is_error { (self.fw_untranslate_error(d, in_name, &ip, now).unwrap_or(ip), None) } else { self.fw_untranslate(d, in_name, ip, now) };
        // Traffic for the firewall itself: only on the interface it arrived on, and ICMP only
        // as the interface's `icmp` rules allow.
        let owner = l3_ifaces(d).into_iter().find(|i| i.net.addr == ip.dst);
        let to_me = ip.dst == Ipv4Addr::BROADCAST || (ifc.net.prefix < 31 && ip.dst == ifc.net.broadcast());
        if owner.is_some() || to_me {
            if owner.is_some_and(|o| o.name != ifc.name) {
                return Err(format!("{} belongs to another of the firewall's interfaces; it only answers on the one a packet arrives on", ip.dst));
            }
            if let Some(ty) = icmp_type(&ip) {
                let (ok, hit) = conf.icmp_allowed(in_name, ip.src, ty);
                if hit.is_some() || !ok {
                    let st = &mut self.st(d.id).fw;
                    let n = st.icmp_hits.entry((in_name.to_string(), hit.unwrap_or(usize::MAX))).or_insert(0);
                    *n = n.saturating_add(1);
                }
                if !ok {
                    let why = match hit {
                        Some(i) => format!("denied by icmp rule {} on {in_name}", i + 1),
                        None => format!("denied: no icmp rule on {in_name} permits it"),
                    };
                    self.log(d.id, &format!("Firewall: icmp from {} to {} (type {ty}) dropped, {why}", ip.src, ip.dst));
                    return Err(why);
                }
            }
            return self.local_in(t, d, ifc, ip);
        }
        let decrement = conf.decrements_ttl();
        if decrement && ip.ttl <= 1 {
            self.icmp_error(t, d, ifc, &ip, Icmp::TimeExceeded { original: Icmp::quote(payload) });
            return Err("TTL expired in transit".into());
        }
        let Some((out, nh)) = self.route(t, d, ip.dst) else {
            self.icmp_error(t, d, ifc, &ip, Icmp::Unreachable { code: netlabcraft_wire::UNREACH_NET, original: Icmp::quote(payload) });
            return Err(format!("no route to {}", ip.dst));
        };
        let Some((out_name, out_level)) = conf.zone(&out.name) else {
            return Err(format!("{} (the way to {}) has no nameif", out.name, ip.dst));
        };
        if out.name == ifc.name && !conf.same_security_intra {
            let why = format!("would leave by {in_name}, the interface it came in on (same-security-traffic permit intra-interface allows that)");
            self.log(d.id, &format!("Firewall: {} → {} dropped, {why}", ip.src, ip.dst));
            return Err(why);
        }
        let flow = flow_of(&ip);
        let size = u64::try_from(ip.payload.len()).unwrap_or(u64::MAX).saturating_add(20);
        let inspect_icmp = conf.inspects("icmp");
        let st = &mut self.st(d.id).fw;
        let tracked = if let Some(q) = icmp_error(&ip).and_then(|(_, _, o)| quoted_flow(&o)) {
            // An error passes when it is about a tracked connection, and never opens one.
            let known = st.conns.iter_mut().find(|c| c.forward(&q) || c.reverse(&q));
            if let Some(c) = known {
                c.last_us = now;
                st.icmp_errors = st.icmp_errors.saturating_add(1);
                Some(false)
            } else {
                None
            }
        } else {
            match st.conns.iter_mut().find(|c| c.forward(&flow) || c.reverse(&flow)) {
                Some(c) => {
                    c.last_us = now;
                    c.packets = c.packets.saturating_add(1);
                    c.bytes = c.bytes.saturating_add(size);
                    if flow.protocol == PROTO_TCP && Tcp::decode(&ip.payload, ip.src, ip.dst).is_ok_and(|s| s.flags & (tcp::FIN | tcp::RST) != 0) {
                        c.closing = true;
                    }
                    if flow.protocol == PROTO_ICMP {
                        st.inspected_icmp = st.inspected_icmp.saturating_add(1);
                    }
                    Some(c.forward(&flow))
                }
                None => None,
            }
        };
        let forward = match tracked {
            Some(f) => f,
            None => {
                if let Err(why) = self.fw_admit(d, in_name, in_level, out_name, out_level, &ip, &flow) {
                    let st = &mut self.st(d.id).fw;
                    st.denied = st.denied.saturating_add(1);
                    let msg = format!(
                        "Firewall: {} {}:{} → {}:{} in on {in_name} dropped, {why}",
                        proto_name(flow.protocol),
                        flow.src,
                        flow.src_port,
                        flow.dst,
                        flow.dst_port
                    );
                    self.log(d.id, &msg);
                    return Err(why);
                }
                // Untracked ICMP (no `inspect icmp`, or an error): checked packet by packet.
                let echo_request = matches!(icmp_type(&ip), Some(8));
                if is_error || (flow.protocol == PROTO_ICMP && !inspect_icmp) {
                    echo_request
                } else {
                    let st = &mut self.st(d.id).fw;
                    if flow.protocol == PROTO_ICMP {
                        st.inspected_icmp = st.inspected_icmp.saturating_add(1);
                    }
                    if st.conns.len() >= MAX_CONNS {
                        st.conns.remove(0);
                    }
                    st.conns.push(Conn {
                        protocol: flow.protocol,
                        src: flow.src,
                        src_port: flow.src_port,
                        dst: flow.dst,
                        dst_port: flow.dst_port,
                        from_if: in_name.to_string(),
                        to_if: out_name.to_string(),
                        started_us: now,
                        last_us: now,
                        packets: 1,
                        bytes: size,
                        closing: false,
                    });
                    true
                }
            }
        };
        let fwd = if decrement { Ipv4 { ttl: ip.ttl - 1, ..ip } } else { ip };
        let mut fwd = self.fw_translate(d, in_name, out_name, &out, fwd, forward, now)?;
        if let Some(src) = src_back {
            fwd = rewrite(&fwd, true, src, None);
        }
        self.transmit(t, d, &out, nh, fwd.encode())
    }

    /// Whether a packet may open a connection: TCP must start with a SYN; then the access
    /// list on the arriving interface decides, or, without one, the security levels; an access
    /// list bound outbound on the leaving interface must agree too.
    #[allow(clippy::too_many_arguments)]
    fn fw_admit(&mut self, d: &Device, in_name: &str, in_level: u8, out_name: &str, out_level: u8, ip: &Ipv4, flow: &Flow) -> Result<(), String> {
        let conf = &d.firewall;
        if flow.protocol == PROTO_TCP && !Tcp::decode(&ip.payload, ip.src, ip.dst).is_ok_and(|s| s.flags & tcp::SYN != 0 && s.flags & tcp::ACK == 0) {
            return Err("denied: a TCP segment of no tracked connection (a connection starts with SYN)".into());
        }
        let ports = matches!(flow.protocol, PROTO_TCP | PROTO_UDP);
        let key = FlowKey {
            protocol: flow.protocol,
            src: flow.src,
            dst: flow.dst,
            src_port: ports.then_some(flow.src_port),
            dst_port: ports.then_some(flow.dst_port),
            icmp_type: icmp_type(ip),
        };
        if let Some(acl) = conf.group_for(in_name).and_then(|n| conf.acl(n)) {
            let (permit, hit) = acl.evaluate(conf, &key);
            let st = self.st(d.id);
            let n = st.acl_hits.entry((acl.name.clone(), hit.unwrap_or(usize::MAX))).or_insert(0);
            *n = n.saturating_add(1);
            match (permit, hit) {
                (true, _) => {}
                (false, Some(i)) => return Err(format!("denied by access-group {} on {in_name} (rule {})", acl.name, i + 1)),
                (false, None) => return Err(format!("denied by access-group {} on {in_name} (no rule matched)", acl.name)),
            }
        } else if in_name.eq_ignore_ascii_case(out_name) {
            // Back out the same interface: `same-security-traffic permit intra-interface` was
            // checked before.
        } else if in_level == out_level && !conf.same_security {
            return Err(format!("denied: {in_name} and {out_name} are both at security level {in_level} (same-security-traffic is not permitted)"));
        } else if in_level < out_level {
            return Err(format!("denied: from security level {in_level} to {out_level} without an access rule"));
        }
        if let Some(acl) = conf.group_out_for(out_name).and_then(|n| conf.acl(n)) {
            let (permit, hit) = acl.evaluate(conf, &key);
            let st = self.st(d.id);
            let n = st.acl_hits.entry((acl.name.clone(), hit.unwrap_or(usize::MAX))).or_insert(0);
            *n = n.saturating_add(1);
            match (permit, hit) {
                (true, _) => {}
                (false, Some(i)) => return Err(format!("denied by access-group {} out on {out_name} (rule {})", acl.name, i + 1)),
                (false, None) => return Err(format!("denied by access-group {} out on {out_name} (no rule matched)", acl.name)),
            }
        }
        Ok(())
    }

    /// Forgets idle connections and unused translations.
    fn fw_expire(&mut self, d: &Device, now: u64) {
        let timeouts = d.firewall.timeouts;
        let st = &mut self.st(d.id).fw;
        st.conns.retain(|c| now.saturating_sub(c.last_us) < c.idle_limit_us(&timeouts));
        let conns = std::mem::take(&mut st.conns);
        st.xlates.retain(|x| {
            now.saturating_sub(x.last_us) < XLATE_IDLE_US
                || conns.iter().any(|c| c.protocol == x.protocol && c.src == x.real && c.src_port == x.real_port && c.to_if == x.mapped_if)
        });
        st.conns = conns;
    }

    /// A packet arriving: its real destination back (a translation it answers, a static rule
    /// it is addressed through, a twice NAT destination), and the source it must leave with
    /// when a twice NAT rule hides its real one.
    fn fw_untranslate(&mut self, d: &Device, in_name: &str, ip: Ipv4, now: u64) -> (Ipv4, Option<Ipv4Addr>) {
        let conf = &d.firewall;
        let id = flow_id(&ip, false);
        let st = &mut self.st(d.id).fw;
        let mut back = None;
        if let Some(x) = st.xlates.iter_mut().find(|x| {
            x.mapped_if.eq_ignore_ascii_case(in_name)
                && x.mapped == ip.dst
                && x.protocol == ip.protocol
                && (!has_ports(ip.protocol) || id == Some(x.mapped_port))
        }) {
            x.last_us = now;
            let port = has_ports(ip.protocol).then_some(x.real_port);
            let label = x.object.clone();
            bump(&mut st.nat_unhits, label);
            back = Some(rewrite(&ip, false, x.real, port));
        }
        // Twice NAT, static source, seen from the mapped side.
        if back.is_none() {
            for (i, r) in conf.twice.iter().enumerate() {
                if r.dynamic || !r.mapped_if.eq_ignore_ascii_case(in_name) || !conf.addr_in(&r.src_mapped, ip.dst) {
                    continue;
                }
                if r.dst.as_ref().is_some_and(|(_, real_dst)| !conf.addr_in(real_dst, ip.src)) {
                    continue;
                }
                let Some(real) = conf.map_between(&r.src_mapped, &r.src_real, ip.dst) else { continue };
                bump(&mut self.st(d.id).fw.nat_unhits, twice_label(i));
                back = Some(rewrite(&ip, false, real, None));
                break;
            }
        }
        // Twice NAT destinations, seen from the real side: the mapped destination becomes the real one.
        if back.is_none() {
            for r in &conf.twice {
                let Some((mapped_dst, real_dst)) = &r.dst else { continue };
                let side = r.real_if.eq_ignore_ascii_case(in_name) || r.real_if.eq_ignore_ascii_case("any");
                if side
                    && conf.addr_in(&r.src_real, ip.src)
                    && let Some(real) = conf.map_between(mapped_dst, real_dst, ip.dst)
                {
                    back = Some(rewrite(&ip, false, real, None));
                    break;
                }
            }
        }
        if back.is_none() {
            back = conf
                .objects
                .iter()
                .filter(|o| o.nat.as_ref().is_some_and(|n| n.mapped_if.eq_ignore_ascii_case(in_name)))
                .find_map(|o| o.static_unmap(ip.dst).map(|real| (o.name.clone(), real)))
                .map(|(name, real)| {
                    bump(&mut self.st(d.id).fw.nat_unhits, name);
                    rewrite(&ip, false, real, None)
                });
        }
        let ip = back.unwrap_or(ip);
        // A reply from a twice NAT rule's real destination leaves with the mapped one.
        let src = conf.twice.iter().find_map(|r| {
            let (mapped_dst, real_dst) = r.dst.as_ref()?;
            (r.mapped_if.eq_ignore_ascii_case(in_name) && conf.addr_in(&r.src_real, ip.dst))
                .then(|| conf.map_between(real_dst, mapped_dst, ip.src))
                .flatten()
        });
        (ip, src)
    }

    /// An ICMP error about a packet the firewall translated on its way out: sent to the real
    /// host, with the quoted packet's source (and port or echo id) translated back too.
    fn fw_untranslate_error(&mut self, d: &Device, in_name: &str, ip: &Ipv4, now: u64) -> Option<Ipv4> {
        let (te, code, original) = icmp_error(ip)?;
        let (_, id_at, proto) = quoted_id_at(&original)?;
        let qsrc = addr_at(&original, 12)?;
        let qid = u16_at(&original, id_at)?;
        let conf = &d.firewall;
        let st = &mut self.st(d.id).fw;
        let found = st
            .xlates
            .iter_mut()
            .find(|x| {
                x.mapped_if.eq_ignore_ascii_case(in_name) && x.mapped == qsrc && x.protocol == proto && (!has_ports(proto) || x.mapped_port == qid)
            })
            .map(|x| {
                x.last_us = now;
                (x.real, has_ports(proto).then_some(x.real_port))
            })
            .or_else(|| {
                conf.objects
                    .iter()
                    .filter(|o| o.nat.as_ref().is_some_and(|n| n.mapped_if.eq_ignore_ascii_case(in_name)))
                    .find_map(|o| o.static_unmap(qsrc))
                    .map(|real| (real, None))
            })
            .or_else(|| {
                conf.twice
                    .iter()
                    .filter(|r| !r.dynamic && r.mapped_if.eq_ignore_ascii_case(in_name))
                    .find_map(|r| conf.map_between(&r.src_mapped, &r.src_real, qsrc))
                    .map(|real| (real, None))
            });
        let (real, port) = found?;
        let original = requote(original, 12, real, port.map(|p| (id_at, p)))?;
        let dst = if ip.dst == qsrc { real } else { ip.dst };
        Some(Ipv4 { dst, payload: error_with(te, code, original), ..ip.clone() })
    }

    /// The source translated on the way out: twice NAT rules first, in order, then network
    /// objects (static rules before dynamic ones). Static rules work in both directions,
    /// dynamic ones only for a connection's first direction.
    #[allow(clippy::too_many_arguments)]
    fn fw_translate(&mut self, d: &Device, in_name: &str, out_name: &str, out: &L3If, ip: Ipv4, forward: bool, now: u64) -> Result<Ipv4, String> {
        let applies = |real_if: &str, mapped_if: &str| {
            (real_if.eq_ignore_ascii_case(in_name) || real_if.eq_ignore_ascii_case("any")) && mapped_if.eq_ignore_ascii_case(out_name)
        };
        let conf = &d.firewall;
        // Twice NAT.
        for (i, r) in conf.twice.iter().enumerate() {
            if !applies(&r.real_if, &r.mapped_if) || !conf.addr_in(&r.src_real, ip.src) {
                continue;
            }
            if r.dst.as_ref().is_some_and(|(_, real_dst)| !conf.addr_in(real_dst, ip.dst)) {
                continue;
            }
            if !r.dynamic {
                let Some(mapped) = conf.map_between(&r.src_real, &r.src_mapped, ip.src) else { continue };
                bump(&mut self.st(d.id).fw.nat_hits, twice_label(i));
                return Ok(self.fw_map_src(ip, mapped));
            }
            if !forward {
                return Ok(ip);
            }
            let mapped = if r.src_mapped.eq_ignore_ascii_case("interface") {
                out.net.addr
            } else {
                match conf.object(&r.src_mapped).and_then(|o| o.addr) {
                    Some(ObjAddr::Host(h)) => h,
                    Some(ObjAddr::Subnet(n)) => Ipv4Addr::from(u32::from(n.addr).saturating_add(1)),
                    None => continue,
                }
            };
            return self.fw_pat(d, in_name, out_name, ip, mapped, twice_label(i), now);
        }
        let objects = &conf.objects;
        // Static rules first.
        if let Some((name, mapped)) = objects.iter().find_map(|o| {
            let n = o.nat.as_ref()?;
            if !applies(&n.real_if, &n.mapped_if) {
                return None;
            }
            o.static_map(ip.src).map(|m| (o.name.clone(), m))
        }) {
            bump(&mut self.st(d.id).fw.nat_hits, name);
            return Ok(self.fw_map_src(ip, mapped));
        }
        if !forward {
            return Ok(ip);
        }
        let Some((name, kind)) = objects.iter().find_map(|o| {
            let n = o.nat.as_ref()?;
            let dynamic = matches!(n.kind, NatKind::DynamicInterface | NatKind::DynamicAddress(_));
            (dynamic && applies(&n.real_if, &n.mapped_if) && o.addr.is_some_and(|a| a.contains(ip.src))).then(|| (o.name.clone(), n.kind))
        }) else {
            return Ok(ip);
        };
        let mapped = match kind {
            NatKind::DynamicAddress(a) => a,
            _ => out.net.addr,
        };
        self.fw_pat(d, in_name, out_name, ip, mapped, name, now)
    }

    /// A one-to-one source translation; an ICMP error's quoted destination (the same host, as
    /// the packet it answers saw it) follows.
    fn fw_map_src(&mut self, ip: Ipv4, mapped: Ipv4Addr) -> Ipv4 {
        let real = ip.src;
        let mut out = rewrite(&ip, true, mapped, None);
        if let Some((te, code, original)) = icmp_error(&ip)
            && addr_at(&original, 16) == Some(real)
            && let Some(q) = requote(original, 16, mapped, None)
        {
            out.payload = error_with(te, code, q);
        }
        out
    }

    /// Port translation of the source to `mapped`, reusing the flow's translation if it has one.
    #[allow(clippy::too_many_arguments)]
    fn fw_pat(&mut self, d: &Device, in_name: &str, out_name: &str, ip: Ipv4, mapped: Ipv4Addr, rule: String, now: u64) -> Result<Ipv4, String> {
        let id = flow_id(&ip, true);
        let real_port = id.unwrap_or(0);
        let st = &mut self.st(d.id).fw;
        if let Some(x) = st.xlates.iter_mut().find(|x| {
            x.protocol == ip.protocol
                && x.real == ip.src
                && x.real_port == real_port
                && x.mapped_if.eq_ignore_ascii_case(out_name)
                && x.mapped == mapped
        }) {
            x.last_us = now;
            let port = has_ports(ip.protocol).then_some(x.mapped_port);
            return Ok(rewrite(&ip, true, mapped, port));
        }
        let taken = |p: u16, xs: &[Xlate]| xs.iter().any(|x| x.protocol == ip.protocol && x.mapped == mapped && x.mapped_port == p);
        let mapped_port = if !has_ports(ip.protocol) || !taken(real_port, &st.xlates) {
            real_port
        } else {
            (1024..=u16::MAX).find(|p| !taken(*p, &st.xlates)).ok_or_else(|| format!("no free port on {mapped} for translation"))?
        };
        if st.xlates.len() >= MAX_XLATES {
            st.xlates.remove(0);
        }
        st.xlates.push(Xlate {
            protocol: ip.protocol,
            real: ip.src,
            real_port,
            mapped,
            mapped_port,
            real_if: in_name.to_string(),
            mapped_if: out_name.to_string(),
            object: rule.clone(),
            last_us: now,
        });
        bump(&mut st.nat_hits, rule);
        let port = has_ports(ip.protocol).then_some(mapped_port);
        Ok(rewrite(&ip, true, mapped, port))
    }

    /// Whether the firewall answers ARP for an address on an interface: a static or
    /// dynamic-address NAT rule maps to it there, or a twice NAT rule uses it there.
    pub(crate) fn fw_owns(d: &Device, ifc: &L3If, a: Ipv4Addr) -> bool {
        let conf = &d.firewall;
        let Some((name, _)) = conf.zone(&ifc.name) else { return false };
        if a == ifc.net.addr {
            return false;
        }
        let in_object = |n: &str| conf.object(n).and_then(|o| o.addr).is_some_and(|o| o.contains(a));
        conf.objects.iter().any(|o| {
            o.nat.as_ref().is_some_and(|n| {
                n.mapped_if.eq_ignore_ascii_case(name)
                    && match n.kind {
                        NatKind::DynamicAddress(m) => m == a,
                        NatKind::Static(_) => o.static_unmap(a).is_some(),
                        NatKind::DynamicInterface => false,
                    }
            })
        }) || conf.twice.iter().any(|r| {
            (r.mapped_if.eq_ignore_ascii_case(name) && !r.src_mapped.eq_ignore_ascii_case("interface") && in_object(&r.src_mapped))
                || (r.real_if.eq_ignore_ascii_case(name) && r.dst.as_ref().is_some_and(|(m, _)| in_object(m)))
        })
    }

    /// The firewall's connections and translations now (idle ones forgotten first).
    pub fn fw_state(&mut self, d: &Device) -> FwState {
        let now = self.now;
        self.fw_expire(d, now);
        self.state.get(&d.id).map(|s| s.fw.clone()).unwrap_or_default()
    }

    /// `clear conn`.
    pub fn fw_clear_conns(&mut self, d: netlabcraft_core::DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.fw.conns.clear();
        }
    }

    /// `clear xlate`: the dynamic translations, and the connections that used them.
    pub fn fw_clear_xlates(&mut self, d: netlabcraft_core::DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            let xs = std::mem::take(&mut st.fw.xlates);
            st.fw.conns.retain(|c| !xs.iter().any(|x| x.protocol == c.protocol && x.real == c.src && x.real_port == c.src_port));
        }
    }

    /// The DHCP pool the firewall serves on an interface: the `dhcpd address` range of its
    /// nameif, when `dhcpd enable` is on there.
    pub(crate) fn fw_dhcp_pool(d: &Device, ifc: &L3If) -> Result<(netlabcraft_model::DhcpPool, Vec<Ipv4Addr>), String> {
        let conf = &d.firewall;
        let (name, _) = conf.zone(&ifc.name).ok_or_else(|| format!("{} has no nameif", ifc.name))?;
        let range = conf
            .dhcpd
            .iter()
            .find(|r| r.nameif.eq_ignore_ascii_case(name) && r.enabled)
            .ok_or_else(|| format!("the DHCP server is not enabled on {name}"))?;
        let pool = netlabcraft_model::DhcpPool {
            name: format!("dhcpd {name}"),
            network: Some(ifc.net.network()),
            default_router: Some(ifc.net.addr),
            dns: conf.dhcpd_dns,
        };
        Ok((pool, range.addresses().filter(|a| ifc.net.contains(*a) && *a != ifc.net.addr).collect()))
    }
}
