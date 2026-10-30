//! The firewall appliance's data path: every routed packet is matched against the connection
//! table; a packet that starts a connection must be allowed by the security levels of the two
//! interfaces or by the access list bound to the interface it arrived on. Replies of a tracked
//! connection always pass. Object NAT translates on the way through and is undone for replies.
//!
//! Connections and translations live in simulated time: an idle connection is forgotten after
//! the firewall's timeout for its protocol, a dynamic translation once no connection uses it
//! and it has been idle for [`XLATE_IDLE_US`].

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use packetcraft_model::Device;
use packetcraft_model::Topology;
use packetcraft_model::firewall::{FlowKey, NatKind};
use packetcraft_wire::tcp::{self, Tcp};
use packetcraft_wire::{Icmp, Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP, Udp};

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
    pub fn idle_limit_us(&self, t: &packetcraft_model::firewall::Timeouts) -> u64 {
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
    /// The network object whose rule made it.
    pub object: String,
    pub last_us: u64,
}

/// A firewall's dynamic state.
#[derive(Debug, Clone, Default)]
pub struct FwState {
    pub conns: Vec<Conn>,
    pub xlates: Vec<Xlate>,
    /// Translations made per network object (`show nat`).
    pub nat_hits: BTreeMap<String, u64>,
    /// Packets dropped by policy.
    pub denied: u64,
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
        let ip = self.fw_untranslate(d, in_name, &ip, now).unwrap_or(ip);
        // Traffic for the firewall itself: only on the interface it arrived on.
        let owner = l3_ifaces(d).into_iter().find(|i| i.net.addr == ip.dst);
        if let Some(o) = owner {
            if o.name != ifc.name {
                return Err(format!("{} belongs to another of the firewall's interfaces; it only answers on the one a packet arrives on", ip.dst));
            }
            return self.local_in(t, d, ifc, ip);
        }
        if ip.dst == Ipv4Addr::BROADCAST || (ifc.net.prefix < 31 && ip.dst == ifc.net.broadcast()) {
            return self.local_in(t, d, ifc, ip);
        }
        if ip.ttl <= 1 {
            self.icmp_error(t, d, ifc, &ip, Icmp::TimeExceeded { original: Icmp::quote(payload) });
            return Err("TTL expired in transit".into());
        }
        let Some((out, nh)) = self.route(t, d, ip.dst) else {
            self.icmp_error(t, d, ifc, &ip, Icmp::Unreachable { code: packetcraft_wire::UNREACH_NET, original: Icmp::quote(payload) });
            return Err(format!("no route to {}", ip.dst));
        };
        let Some((out_name, out_level)) = conf.zone(&out.name) else {
            return Err(format!("{} (the way to {}) has no nameif", out.name, ip.dst));
        };
        if out.name == ifc.name {
            return Err(format!("would leave by {in_name}, the interface it came in on; the firewall doesn't send traffic back out the same way"));
        }
        let flow = flow_of(&ip);
        let size = u64::try_from(ip.payload.len()).unwrap_or(u64::MAX).saturating_add(20);
        let st = &mut self.st(d.id).fw;
        let forward = match st.conns.iter_mut().find(|c| c.forward(&flow) || c.reverse(&flow)) {
            Some(c) => {
                c.last_us = now;
                c.packets = c.packets.saturating_add(1);
                c.bytes = c.bytes.saturating_add(size);
                if flow.protocol == PROTO_TCP && Tcp::decode(&ip.payload, ip.src, ip.dst).is_ok_and(|s| s.flags & (tcp::FIN | tcp::RST) != 0) {
                    c.closing = true;
                }
                c.forward(&flow)
            }
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
                let st = &mut self.st(d.id).fw;
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
        };
        let fwd = Ipv4 { ttl: ip.ttl - 1, ..ip };
        let fwd = self.fw_translate(d, in_name, out_name, &out, fwd, forward, now)?;
        self.transmit(t, d, &out, nh, fwd.encode())
    }

    /// Whether a packet may open a connection: TCP must start with a SYN; then the access
    /// list on the arriving interface decides, or, without one, the security levels.
    #[allow(clippy::too_many_arguments)]
    fn fw_admit(&mut self, d: &Device, in_name: &str, in_level: u8, out_name: &str, out_level: u8, ip: &Ipv4, flow: &Flow) -> Result<(), String> {
        let conf = &d.firewall;
        if flow.protocol == PROTO_TCP && !Tcp::decode(&ip.payload, ip.src, ip.dst).is_ok_and(|s| s.flags & tcp::SYN != 0 && s.flags & tcp::ACK == 0) {
            return Err("denied: a TCP segment of no tracked connection (a connection starts with SYN)".into());
        }
        if let Some(acl) = conf.group_for(in_name).and_then(|n| conf.acl(n)) {
            let ports = matches!(flow.protocol, PROTO_TCP | PROTO_UDP);
            let key = FlowKey {
                protocol: flow.protocol,
                src: flow.src,
                dst: flow.dst,
                src_port: ports.then_some(flow.src_port),
                dst_port: ports.then_some(flow.dst_port),
            };
            let (permit, hit) = acl.evaluate(conf, &key);
            let st = self.st(d.id);
            let n = st.acl_hits.entry((acl.name.clone(), hit.unwrap_or(usize::MAX))).or_insert(0);
            *n = n.saturating_add(1);
            return match (permit, hit) {
                (true, _) => Ok(()),
                (false, Some(i)) => Err(format!("denied by access-group {} on {in_name} (rule {})", acl.name, i + 1)),
                (false, None) => Err(format!("denied by access-group {} on {in_name} (no rule matched)", acl.name)),
            };
        }
        if in_level > out_level || (in_level == out_level && conf.same_security) {
            return Ok(());
        }
        if in_level == out_level {
            return Err(format!("denied: {in_name} and {out_name} are both at security level {in_level} (same-security-traffic is not permitted)"));
        }
        Err(format!("denied: from security level {in_level} to {out_level} without an access rule"))
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

    /// A packet arriving on the mapped side of a translation, with its real destination back.
    fn fw_untranslate(&mut self, d: &Device, in_name: &str, ip: &Ipv4, now: u64) -> Option<Ipv4> {
        let id = flow_id(ip, false);
        let st = &mut self.st(d.id).fw;
        if let Some(x) = st.xlates.iter_mut().find(|x| {
            x.mapped_if.eq_ignore_ascii_case(in_name)
                && x.mapped == ip.dst
                && x.protocol == ip.protocol
                && (!has_ports(ip.protocol) || id == Some(x.mapped_port))
        }) {
            x.last_us = now;
            let port = has_ports(ip.protocol).then_some(x.real_port);
            return Some(rewrite(ip, false, x.real, port));
        }
        d.firewall
            .objects
            .iter()
            .filter(|o| o.nat.as_ref().is_some_and(|n| n.mapped_if.eq_ignore_ascii_case(in_name)))
            .find_map(|o| o.static_unmap(ip.dst))
            .map(|real| rewrite(ip, false, real, None))
    }

    /// The source translated on the way out, by the first network object whose rule covers it:
    /// static rules in both directions, dynamic ones only for a connection's first direction.
    #[allow(clippy::too_many_arguments)]
    fn fw_translate(&mut self, d: &Device, in_name: &str, out_name: &str, out: &L3If, ip: Ipv4, forward: bool, now: u64) -> Result<Ipv4, String> {
        let applies = |real_if: &str, mapped_if: &str| {
            (real_if.eq_ignore_ascii_case(in_name) || real_if.eq_ignore_ascii_case("any")) && mapped_if.eq_ignore_ascii_case(out_name)
        };
        let objects = &d.firewall.objects;
        // Static rules first.
        if let Some((name, mapped)) = objects.iter().find_map(|o| {
            let n = o.nat.as_ref()?;
            if !applies(&n.real_if, &n.mapped_if) {
                return None;
            }
            o.static_map(ip.src).map(|m| (o.name.clone(), m))
        }) {
            let st = &mut self.st(d.id).fw;
            *st.nat_hits.entry(name).or_insert(0) += 1;
            return Ok(rewrite(&ip, true, mapped, None));
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
            object: name.clone(),
            last_us: now,
        });
        *st.nat_hits.entry(name).or_insert(0) += 1;
        let port = has_ports(ip.protocol).then_some(mapped_port);
        Ok(rewrite(&ip, true, mapped, port))
    }

    /// Whether the firewall answers ARP for an address on an interface: a static or
    /// dynamic-address NAT rule maps to it there.
    pub(crate) fn fw_owns(d: &Device, ifc: &L3If, a: Ipv4Addr) -> bool {
        let Some((name, _)) = d.firewall.zone(&ifc.name) else { return false };
        d.firewall.objects.iter().any(|o| {
            o.nat.as_ref().is_some_and(|n| {
                n.mapped_if.eq_ignore_ascii_case(name)
                    && match n.kind {
                        NatKind::DynamicAddress(m) => m == a,
                        NatKind::Static(_) => o.static_unmap(a).is_some(),
                        NatKind::DynamicInterface => false,
                    }
            })
        })
    }

    /// The firewall's connections and translations now (idle ones forgotten first).
    pub fn fw_state(&mut self, d: &Device) -> FwState {
        let now = self.now;
        self.fw_expire(d, now);
        self.state.get(&d.id).map(|s| s.fw.clone()).unwrap_or_default()
    }

    /// `clear conn`.
    pub fn fw_clear_conns(&mut self, d: packetcraft_core::DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.fw.conns.clear();
        }
    }

    /// `clear xlate`: the dynamic translations, and the connections that used them.
    pub fn fw_clear_xlates(&mut self, d: packetcraft_core::DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            let xs = std::mem::take(&mut st.fw.xlates);
            st.fw.conns.retain(|c| !xs.iter().any(|x| x.protocol == c.protocol && x.real == c.src && x.real_port == c.src_port));
        }
    }

    /// The DHCP pool the firewall serves on an interface: the `dhcpd address` range of its
    /// nameif, when `dhcpd enable` is on there.
    pub(crate) fn fw_dhcp_pool(d: &Device, ifc: &L3If) -> Result<(packetcraft_model::DhcpPool, Vec<Ipv4Addr>), String> {
        let conf = &d.firewall;
        let (name, _) = conf.zone(&ifc.name).ok_or_else(|| format!("{} has no nameif", ifc.name))?;
        let range = conf
            .dhcpd
            .iter()
            .find(|r| r.nameif.eq_ignore_ascii_case(name) && r.enabled)
            .ok_or_else(|| format!("the DHCP server is not enabled on {name}"))?;
        let pool = packetcraft_model::DhcpPool {
            name: format!("dhcpd {name}"),
            network: Some(ifc.net.network()),
            default_router: Some(ifc.net.addr),
            dns: conf.dhcpd_dns,
        };
        Ok((pool, range.addresses().filter(|a| ifc.net.contains(*a) && *a != ifc.net.addr).collect()))
    }
}
