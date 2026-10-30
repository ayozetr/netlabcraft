//! IPv6: interfaces, neighbour discovery, routing, echo and stateless autoconfiguration.
//!
//! Same shape as the IPv4 side: neighbour solicitations stand in for ARP (sent to the target's
//! solicited-node group), and every device holds packets until the neighbour answers. Routers with `ipv6 unicast-routing` answer router
//! solicitations with the /64 prefixes of the interface, and a host on autoconfiguration
//! builds its address from that prefix and its MAC (EUI-64).

use std::net::{IpAddr, Ipv6Addr};

use netlabcraft_core::{Ipv6Net, MacAddr, VlanId, eui64, link_local};
use netlabcraft_model::{Device, DeviceKind, End, Ipv6Conf, Topology};
use netlabcraft_wire::Frame;
use netlabcraft_wire::dhcp6::{self, Dhcp6};
use netlabcraft_wire::ipv6::{
    ALL_NODES, ALL_ROUTERS, ETHERTYPE_IPV6, Icmp6, Ipv6, PROTO_ICMPV6, PrefixInfo, RA_MANAGED, RA_OTHER, multicast_mac, solicited_node,
};

use crate::{ARP_AGING_US, ArpEntry, IcmpKind, MAX_PENDING, PingOptions, Probe, SVI_PORT, Sim, SimError, port_up, svi_mac};

const ND_WAIT_US: u64 = 2_000_000;
const ND_RETRY_US: u64 = 1_000_000;
const MAX_ND_ENTRIES: usize = 4_096;

/// An IPv6 interface: a routed port, a router's subinterface, a switch's VLAN interface
/// (`port` is `SVI_PORT`), or a host's port with IPv6 on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L3If6 {
    pub port: usize,
    /// The subinterface's or VLAN interface's VLAN.
    pub vlan: Option<u16>,
    pub name: String,
    pub mac: MacAddr,
    pub link_local: Ipv6Addr,
    pub addresses: Vec<Ipv6Net>,
}

impl L3If6 {
    /// Whether `a` is one of this interface's addresses.
    pub fn owns(&self, a: Ipv6Addr) -> bool {
        a == self.link_local || self.addresses.iter().any(|n| n.addr == a)
    }

    /// The source address towards `dst`: link-local for link-local and multicast
    /// destinations, else the first global address (link-local if there is none).
    pub fn source_for(&self, dst: Ipv6Addr) -> Ipv6Addr {
        if Ipv6Net::is_link_local(dst) || dst.is_multicast() {
            return self.link_local;
        }
        self.addresses.iter().find(|n| n.contains(dst)).or(self.addresses.first()).map_or(self.link_local, |n| n.addr)
    }
}

/// Whether a device forwards IPv6: a router, or a multilayer switch routing IPv4 too.
pub fn routes6(d: &Device) -> bool {
    d.ipv6_routing && (d.kind == DeviceKind::Router || (d.kind == DeviceKind::Switch && d.ip_routing))
}

/// The IPv6 settings behind an interface.
pub fn conf6<'a>(d: &'a Device, ifc: &L3If6) -> Option<&'a Ipv6Conf> {
    if ifc.port == SVI_PORT {
        return d.svis.iter().find(|s| Some(s.vlan.get()) == ifc.vlan).map(|s| &s.ipv6);
    }
    let p = d.ports.get(ifc.port)?;
    match ifc.vlan {
        Some(v) => p.subifs.iter().find(|s| s.vlan.map(VlanId::get) == Some(v)).map(|s| &s.ipv6),
        None => Some(&p.ipv6),
    }
}

/// The IPv6 interfaces a device has on and not shut down.
pub fn l3_ifaces6(d: &Device) -> Vec<L3If6> {
    let make = |port: usize, vlan: Option<u16>, name: String, mac: MacAddr, c: &Ipv6Conf| L3If6 {
        port,
        vlan,
        name,
        mac,
        link_local: c.link_local.unwrap_or_else(|| link_local(mac)),
        addresses: c.addresses.clone(),
    };
    match d.kind {
        DeviceKind::Hub => Vec::new(),
        DeviceKind::Switch => {
            let mac = svi_mac(d);
            let mut out: Vec<L3If6> = d
                .svis
                .iter()
                .filter(|s| !s.shutdown && s.ipv6.active())
                .map(|s| make(SVI_PORT, Some(s.vlan.get()), format!("Vlan{}", s.vlan), mac, &s.ipv6))
                .collect();
            for (i, p) in d.ports.iter().enumerate().filter(|(_, p)| p.routed && !p.shutdown && p.ipv6.active()) {
                out.push(make(i, None, p.name.clone(), p.mac, &p.ipv6));
            }
            out
        }
        _ => {
            let mut out = Vec::new();
            for (i, p) in d.ports.iter().enumerate().filter(|(_, p)| !p.shutdown) {
                if p.ipv6.active() {
                    out.push(make(i, None, p.name.clone(), p.mac, &p.ipv6));
                }
                if d.kind == DeviceKind::Router {
                    for s in p.subifs.iter().filter(|s| !s.shutdown && s.ipv6.active()) {
                        if let Some(v) = s.vlan {
                            out.push(make(i, Some(v.get()), format!("{}.{}", p.name, s.number), p.mac, &s.ipv6));
                        }
                    }
                }
            }
            out
        }
    }
}

/// The IPv6 interfaces that are up (cabled, both ends on; a VLAN interface while some port of
/// its VLAN is up).
pub fn up_ifaces6(t: &Topology, d: &Device) -> Vec<L3If6> {
    l3_ifaces6(d)
        .into_iter()
        .filter(|i| {
            if i.port == SVI_PORT {
                i.vlan
                    .and_then(VlanId::new)
                    .is_some_and(|v| (0..d.ports.len()).any(|p| crate::stp::carries(t, d, p, v) && port_up(t, End { device: d.id, port: p })))
            } else {
                port_up(t, End { device: d.id, port: i.port })
            }
        })
        .collect()
}

/// One line of `show ipv6 route`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route6Entry {
    /// `C` connected, `L` local, `S` static; OSPFv3: `O` intra-area, `I` inter-area, `E` external.
    pub code: char,
    pub net: Ipv6Net,
    pub iface: Option<String>,
    pub next_hop: Option<Ipv6Addr>,
}

/// A device's IPv6 routing table: connected and local routes of its up interfaces, the static
/// routes that can be used, then `dynamic` ones (OSPFv3) for networks not already there.
pub fn routing_table6(t: &Topology, d: &Device, dynamic: &[Route6Entry]) -> Vec<Route6Entry> {
    let ifs = up_ifaces6(t, d);
    let mut out = Vec::new();
    for i in &ifs {
        for a in &i.addresses {
            out.push(Route6Entry { code: 'C', net: a.network(), iface: Some(i.name.clone()), next_hop: None });
            out.push(Route6Entry { code: 'L', net: Ipv6Net::new(a.addr, 128), iface: Some(i.name.clone()), next_hop: None });
        }
    }
    for r in &d.routes6 {
        let iface_up = r.iface.as_ref().is_none_or(|n| ifs.iter().any(|i| i.name == *n));
        let reachable = match (&r.iface, r.next_hop) {
            (Some(_), _) => iface_up,
            (None, Some(nh)) => ifs.iter().any(|i| i.addresses.iter().any(|a| a.contains(nh))),
            (None, None) => false,
        };
        if reachable {
            out.push(Route6Entry { code: 'S', net: r.net, iface: r.iface.clone(), next_hop: r.next_hop });
        }
    }
    // Connected and static routes win over OSPF for the same network (administrative distance).
    for r in dynamic {
        if !out.iter().any(|x| x.net == r.net && x.code != 'L') {
            out.push(r.clone());
        }
    }
    out
}

/// Where a packet to `dst` leaves, and the neighbour it is handed to. `via` pins the interface
/// for link-local and multicast destinations (the one a request came in on).
pub fn route6(t: &Topology, d: &Device, dst: Ipv6Addr, via: Option<&L3If6>, dynamic: &[Route6Entry]) -> Option<(L3If6, Ipv6Addr)> {
    let ifs = up_ifaces6(t, d);
    if Ipv6Net::is_link_local(dst) || dst.is_multicast() {
        let out = via.cloned().or_else(|| ifs.first().cloned())?;
        return Some((out, dst));
    }
    if d.kind.is_host() {
        if let Some(i) = ifs.iter().find(|i| i.addresses.iter().any(|a| a.contains(dst))) {
            return Some((i.clone(), dst));
        }
        let gw = d.gateway6?;
        let out = ifs.iter().find(|i| Ipv6Net::is_link_local(gw) || i.addresses.iter().any(|a| a.contains(gw)))?;
        return Some((out.clone(), gw));
    }
    let table = routing_table6(t, d, dynamic);
    let best = table.iter().filter(|r| r.code != 'L' && r.net.contains(dst)).max_by_key(|r| (r.net.prefix, r.code == 'C'))?;
    match (best.code, &best.iface, best.next_hop) {
        ('C', Some(name), _) => ifs.iter().find(|i| i.name == *name).map(|i| (i.clone(), dst)),
        (_, Some(name), Some(nh)) => ifs.iter().find(|i| i.name == *name).map(|i| (i.clone(), nh)),
        (_, Some(name), None) => ifs.iter().find(|i| i.name == *name).map(|i| (i.clone(), dst)),
        (_, None, Some(nh)) => ifs.iter().find(|i| i.addresses.iter().any(|a| a.contains(nh))).map(|i| (i.clone(), nh)),
        _ => None,
    }
}

/// What automatic IPv6 configuration got a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Auto6 {
    pub address: Ipv6Net,
    pub router: Ipv6Addr,
    pub dns: Option<Ipv6Addr>,
    /// How: SLAAC, SLAAC and stateless DHCPv6, or stateful DHCPv6.
    pub how: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct Pending6 {
    next_hop: Ipv6Addr,
    iface: String,
    packet: Vec<u8>,
    since: u64,
}

impl Pending6 {
    pub(crate) fn iface(&self) -> &str {
        &self.iface
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Icmp6Event {
    at: u64,
    from: Ipv6Addr,
    id: u16,
    seq: u16,
    kind: IcmpKind,
}

fn hop_limit(kind: DeviceKind) -> u8 {
    if kind.is_host() { 128 } else { 64 }
}

impl Sim {
    /// Takes an IPv6 frame in on a routed or host port.
    pub(crate) fn ip6_in(&mut self, t: &Topology, d: &Device, in_port: usize, f: &Frame) -> Result<(), String> {
        let ifc = l3_ifaces6(d).into_iter().find(|i| i.port == in_port && i.vlan == f.vlan).ok_or("IPv6 is not enabled on this interface")?;
        self.ip6_on(t, d, &ifc, f)
    }

    /// Takes an IPv6 frame in on a known interface (a switch hands its VLAN interface's here).
    pub(crate) fn ip6_on(&mut self, t: &Topology, d: &Device, ifc: &L3If6, f: &Frame) -> Result<(), String> {
        let ifc = ifc.clone();
        let ip = Ipv6::decode(&f.payload).map_err(|e| e.to_string())?;
        if let Err(e) = self.acl6_check(d, &ifc, &ip, true) {
            self.acl6_refuse(t, d, &ifc, &ip);
            return Err(e);
        }
        let router = routes6(d);
        let all = l3_ifaces6(d);
        let serves = conf6(d, &ifc).is_some_and(|c| c.dhcp_server.is_some() || !c.dhcp_relay.is_empty());
        let ospf6 = d.ospf6.is_some();
        let joined = |g: Ipv6Addr| {
            g == ALL_NODES
                || (router && g == ALL_ROUTERS)
                || (ospf6 && g == netlabcraft_wire::ospf6::ALL_SPF_ROUTERS6)
                || (serves && g == dhcp6::ALL_SERVERS)
                || all.iter().any(|i| i.addresses.iter().map(|n| n.addr).chain([i.link_local]).any(|a| solicited_node(a) == g))
        };
        let for_me = if f.dst.is_group() { f.dst == multicast_mac(ip.dst) && joined(ip.dst) } else { f.dst == ifc.mac };
        if !for_me {
            return Err("not addressed to this device".into());
        }
        if ifc.owns(ip.dst) || all.iter().any(|i| i.addresses.iter().any(|n| n.addr == ip.dst)) || ip.dst.is_multicast() {
            return self.local6_in(t, d, &ifc, f.src, ip);
        }
        if !router {
            return Err(format!("{} is not this device's address", ip.dst));
        }
        if Ipv6Net::is_link_local(ip.src) || Ipv6Net::is_link_local(ip.dst) {
            return Err("link-local packets never leave their link".into());
        }
        if ip.hop_limit <= 1 {
            let raw = ip.encode();
            self.icmp6_error(t, d, &ifc, &ip, Icmp6::TimeExceeded { original: Icmp6::quote(&raw) });
            return Err("hop limit expired in transit".into());
        }
        let dynamic = self.dynamic_routes6(t, d);
        match route6(t, d, ip.dst, None, &dynamic) {
            Some((out, nh)) => {
                if let Err(e) = self.acl6_check(d, &out, &ip, false) {
                    self.acl6_refuse(t, d, &ifc, &ip);
                    return Err(e);
                }
                let fwd = Ipv6 { hop_limit: ip.hop_limit - 1, ..ip };
                self.transmit6(t, d, &out, nh, fwd.encode())
            }
            None => {
                let raw = ip.encode();
                self.icmp6_error(
                    t,
                    d,
                    &ifc,
                    &ip,
                    Icmp6::Unreachable { code: netlabcraft_wire::ipv6::UNREACH6_NO_ROUTE, original: Icmp6::quote(&raw) },
                );
                Err(format!("no IPv6 route to {}", ip.dst))
            }
        }
    }

    fn local6_in(&mut self, t: &Topology, d: &Device, ifc: &L3If6, src_mac: MacAddr, ip: Ipv6) -> Result<(), String> {
        if ip.next_header == netlabcraft_wire::PROTO_UDP {
            return self.udp6_in(t, d, ifc, ip);
        }
        // OSPFv3 is computed; its hellos are taken in quietly.
        if ip.next_header == netlabcraft_wire::ospf6::PROTO_OSPF && d.ospf6.is_some() {
            return Ok(());
        }
        if ip.next_header != PROTO_ICMPV6 {
            return Err(format!("IPv6 next header {} not understood", ip.next_header));
        }
        let msg = Icmp6::decode(&ip.payload, ip.src, ip.dst).map_err(|e| e.to_string())?;
        let now = self.now;
        let router = routes6(d);
        let note = |s: &mut Sim, id: u16, seq: u16, kind: IcmpKind| {
            let st = s.st(d.id);
            if st.icmp6.len() < 256 {
                st.icmp6.push(Icmp6Event { at: now, from: ip.src, id, seq, kind });
            }
        };
        match msg {
            Icmp6::EchoRequest { id, seq, data } => {
                if ip.dst.is_multicast() && ip.dst != ALL_NODES {
                    return Ok(());
                }
                let src = if ip.dst.is_multicast() { ifc.source_for(ip.src) } else { ip.dst };
                let reply = Ipv6 {
                    hop_limit: hop_limit(d.kind),
                    next_header: PROTO_ICMPV6,
                    src,
                    dst: ip.src,
                    payload: Icmp6::EchoReply { id, seq, data }.encode(src, ip.src),
                };
                self.learn_nd(t, d, ifc, ip.src, src_mac);
                self.send_ip6(t, d, reply, Some(ifc))
            }
            Icmp6::EchoReply { id, seq, .. } => {
                note(self, id, seq, IcmpKind::Reply);
                Ok(())
            }
            Icmp6::TimeExceeded { original } => {
                if let Some((id, seq)) = Icmp6::quoted_echo(&original) {
                    note(self, id, seq, IcmpKind::TimeExceeded);
                }
                Ok(())
            }
            Icmp6::Unreachable { code, original } => {
                if let Some((id, seq)) = Icmp6::quoted_echo(&original) {
                    note(self, id, seq, IcmpKind::Unreachable(code));
                }
                Ok(())
            }
            Icmp6::NeighborSolicit { target, mac } => {
                if !ifc.owns(target) {
                    return Err(format!("neighbour solicitation for {target}, not ours"));
                }
                if !ip.src.is_unspecified() {
                    self.learn_nd(t, d, ifc, ip.src, mac.unwrap_or(src_mac));
                }
                let dst = if ip.src.is_unspecified() { ALL_NODES } else { ip.src };
                let na = Icmp6::NeighborAdvert { target, router: routes6(d), solicited: !ip.src.is_unspecified(), mac: Some(ifc.mac) };
                let src = target;
                let packet = Ipv6 { hop_limit: 255, next_header: PROTO_ICMPV6, src, dst, payload: na.encode(src, dst) };
                let to = if dst.is_multicast() { multicast_mac(dst) } else { mac.unwrap_or(src_mac) };
                self.emit6(t, d, ifc, to, packet.encode());
                Ok(())
            }
            Icmp6::NeighborAdvert { target, mac, .. } => {
                self.learn_nd(t, d, ifc, target, mac.unwrap_or(src_mac));
                Ok(())
            }
            Icmp6::RouterSolicit { mac } => {
                if !router {
                    return Err("not an IPv6 router (no `ipv6 unicast-routing`)".into());
                }
                if !ip.src.is_unspecified() {
                    self.learn_nd(t, d, ifc, ip.src, mac.unwrap_or(src_mac));
                }
                let prefixes: Vec<PrefixInfo> =
                    ifc.addresses.iter().filter(|n| n.prefix == 64).map(|n| PrefixInfo { prefix: n.network().addr, len: 64 }).collect();
                let conf = conf6(d, ifc).cloned().unwrap_or_default();
                let flags = (if conf.nd_managed { RA_MANAGED } else { 0 }) | (if conf.nd_other { RA_OTHER } else { 0 });
                let ra = Icmp6::RouterAdvert { hop_limit: 64, flags, lifetime: 1_800, mac: Some(ifc.mac), prefixes };
                let packet = Ipv6 {
                    hop_limit: 255,
                    next_header: PROTO_ICMPV6,
                    src: ifc.link_local,
                    dst: ALL_NODES,
                    payload: ra.encode(ifc.link_local, ALL_NODES),
                };
                self.emit6(t, d, ifc, multicast_mac(ALL_NODES), packet.encode());
                Ok(())
            }
            Icmp6::RouterAdvert { lifetime, mac, prefixes, flags, .. } => {
                if d.kind.is_host() && lifetime > 0 {
                    self.learn_nd(t, d, ifc, ip.src, mac.unwrap_or(src_mac));
                    self.st(d.id).ra = Some((ip.src, prefixes, flags));
                }
                Ok(())
            }
        }
    }

    /// Applies the IPv6 access list on a port, if one is set and exists.
    fn acl6_check(&mut self, d: &Device, ifc: &L3If6, ip: &Ipv6, inbound: bool) -> Result<(), String> {
        let Some(c) = conf6(d, ifc) else { return Ok(()) };
        let Some(name) = (if inbound { &c.filter_in } else { &c.filter_out }) else { return Ok(()) };
        // A filter naming a list that doesn't exist lets everything through.
        let Some(acl) = d.acls6.iter().find(|a| a.name == *name) else { return Ok(()) };
        let nd = ip.next_header == PROTO_ICMPV6 && ip.payload.first().is_some_and(|t| (133..=137).contains(t));
        let key = netlabcraft_model::acl::Packet6Key { next_header: ip.next_header, src: ip.src, dst: ip.dst, src_port: None, dst_port: None, nd };
        let (permit, hit) = acl.evaluate(&key);
        if hit.is_some() || !permit {
            *self.st(d.id).acl_hits.entry((format!("ipv6 {name}"), hit.unwrap_or(usize::MAX))).or_insert(0) += 1;
        }
        if permit { Ok(()) } else { Err(format!("denied by IPv6 access list {name} ({})", if inbound { "in" } else { "out" })) }
    }

    /// Tells the sender a filter refused its packet (administratively prohibited).
    fn acl6_refuse(&mut self, t: &Topology, d: &Device, ifc: &L3If6, ip: &Ipv6) {
        let raw = ip.encode();
        self.icmp6_error(t, d, ifc, ip, Icmp6::Unreachable { code: 1, original: Icmp6::quote(&raw) });
    }

    fn learn_nd(&mut self, t: &Topology, d: &Device, ifc: &L3If6, ip: Ipv6Addr, mac: MacAddr) {
        let now = self.now;
        let st = self.st(d.id);
        if st.nd.len() >= MAX_ND_ENTRIES && !st.nd.contains_key(&ip) {
            return;
        }
        st.nd.insert(ip, ArpEntry { mac, learned_us: now, iface: ifc.name.clone() });
        st.nd_asked.remove(&ip);
        // Packets that waited for this neighbour go now.
        let (ready, rest): (Vec<Pending6>, Vec<Pending6>) =
            std::mem::take(&mut st.pending6).into_iter().partition(|p| p.next_hop == ip && p.iface == ifc.name);
        st.pending6 = rest;
        for p in ready {
            self.emit6(t, d, ifc, mac, p.packet);
        }
    }

    /// Sends a packet out of an interface: tagged on a subinterface, through the switch on a
    /// VLAN interface.
    fn emit6(&mut self, t: &Topology, d: &Device, ifc: &L3If6, to: MacAddr, packet: Vec<u8>) {
        if ifc.port == SVI_PORT {
            let frame = Frame { dst: to, src: ifc.mac, vlan: None, ethertype: ETHERTYPE_IPV6, payload: packet };
            if let Some(v) = ifc.vlan.and_then(VlanId::new) {
                let _ = self.switch_forward(t, d, None, v, frame, 0);
            }
            return;
        }
        let frame = Frame { dst: to, src: ifc.mac, vlan: ifc.vlan, ethertype: ETHERTYPE_IPV6, payload: packet };
        self.emit_frame(t, d, ifc.port, &frame);
    }

    /// Routes and sends a packet this device originates.
    fn send_ip6(&mut self, t: &Topology, d: &Device, ip: Ipv6, via: Option<&L3If6>) -> Result<(), String> {
        let dynamic = self.dynamic_routes6(t, d);
        let (out, nh) = route6(t, d, ip.dst, via, &dynamic).ok_or_else(|| format!("no IPv6 route to {}", ip.dst))?;
        self.transmit6(t, d, &out, nh, ip.encode())
    }

    /// Puts a packet on the wire towards `nh`, finding its MAC with neighbour discovery first.
    fn transmit6(&mut self, t: &Topology, d: &Device, out: &L3If6, nh: Ipv6Addr, packet: Vec<u8>) -> Result<(), String> {
        // Multicast, and anything on a point-to-point serial link, needs no neighbour lookup.
        if nh.is_multicast() || t.is_serial(End { device: d.id, port: out.port }) {
            self.emit6(t, d, out, multicast_mac(nh), packet);
            return Ok(());
        }
        let now = self.now;
        let known = self.st(d.id).nd.get(&nh).filter(|e| now.saturating_sub(e.learned_us) < ARP_AGING_US && e.iface == out.name).map(|e| e.mac);
        if let Some(mac) = known {
            self.emit6(t, d, out, mac, packet);
            return Ok(());
        }
        // Unlike ARP on routers, neighbour discovery holds the packet until the answer (everyone
        // does), so the first IPv6 ping isn't lost.
        let st = self.st(d.id);
        st.pending6.retain(|p| now.saturating_sub(p.since) < ND_WAIT_US);
        if st.pending6.len() < MAX_PENDING {
            st.pending6.push(Pending6 { next_hop: nh, iface: out.name.clone(), packet, since: now });
        }
        let ask = st.nd_asked.get(&nh).is_none_or(|at| now.saturating_sub(*at) >= ND_RETRY_US);
        if ask {
            if st.nd_asked.len() > MAX_ND_ENTRIES {
                st.nd_asked.clear();
            }
            st.nd_asked.insert(nh, now);
            let group = solicited_node(nh);
            let src = out.source_for(nh);
            let ns = Icmp6::NeighborSolicit { target: nh, mac: Some(out.mac) };
            let p = Ipv6 { hop_limit: 255, next_header: PROTO_ICMPV6, src, dst: group, payload: ns.encode(src, group) };
            self.emit6(t, d, out, multicast_mac(group), p.encode());
        }
        Ok(())
    }

    /// Sends an ICMPv6 error about `offending` back to its source (never about an error).
    fn icmp6_error(&mut self, t: &Topology, d: &Device, ifc: &L3If6, offending: &Ipv6, msg: Icmp6) {
        if offending.next_header == PROTO_ICMPV6
            && !matches!(Icmp6::decode(&offending.payload, offending.src, offending.dst), Ok(Icmp6::EchoRequest { .. } | Icmp6::EchoReply { .. }))
        {
            return;
        }
        let src = ifc.source_for(offending.src);
        let err = Ipv6 { hop_limit: hop_limit(d.kind), next_header: PROTO_ICMPV6, src, dst: offending.src, payload: msg.encode(src, offending.src) };
        let _ = self.send_ip6(t, d, err, Some(ifc));
    }

    fn probe6(
        &mut self,
        t: &Topology,
        dev: crate::DeviceId,
        dst: Ipv6Addr,
        hops: u8,
        seq: u16,
        size: usize,
        timeout: u64,
    ) -> Result<Probe, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let ifs = up_ifaces6(t, d);
        if l3_ifaces6(d).is_empty() {
            return Err(SimError::NoSource(format!("{} has no IPv6 address", d.name)));
        }
        if l3_ifaces6(d).iter().any(|i| i.owns(dst)) {
            return Ok(Probe::Reply { from: dst.into(), rtt_us: 0 });
        }
        let dynamic = self.dynamic_routes6(t, d);
        let Some((out, _)) = route6(t, d, dst, None, &dynamic) else {
            if ifs.is_empty() {
                return Ok(Probe::Timeout);
            }
            return Err(SimError::NoSource(format!("{} has no IPv6 route to {dst}", d.name)));
        };
        self.next_echo_id = self.next_echo_id.wrapping_add(1);
        let id = self.next_echo_id;
        let src = out.source_for(dst);
        let ip = Ipv6 {
            hop_limit: hops,
            next_header: PROTO_ICMPV6,
            src,
            dst,
            payload: Icmp6::EchoRequest { id, seq, data: vec![0xab; size.min(1_452)] }.encode(src, dst),
        };
        let sent = self.now;
        if self.send_ip6(t, d, ip, None).is_err() && d.kind != DeviceKind::Router {
            return Ok(Probe::Timeout);
        }
        let matches = |s: &Sim| s.state.get(&dev).is_some_and(|st| st.icmp6.iter().any(|e| e.id == id && e.seq == seq));
        self.advance(t, sent.saturating_add(timeout), matches);
        let st = self.st(dev);
        let ev = st.icmp6.iter().position(|e| e.id == id && e.seq == seq).map(|i| st.icmp6.remove(i));
        st.icmp6.retain(|e| sent.saturating_sub(e.at) < 60_000_000);
        Ok(match ev {
            Some(Icmp6Event { kind: IcmpKind::Reply, from, at, .. }) => Probe::Reply { from: IpAddr::V6(from), rtt_us: at.saturating_sub(sent) },
            Some(Icmp6Event { kind: IcmpKind::TimeExceeded, from, at, .. }) => {
                Probe::TtlExceeded { from: IpAddr::V6(from), rtt_us: at.saturating_sub(sent) }
            }
            Some(Icmp6Event { kind: IcmpKind::Unreachable(code), from, .. }) => Probe::Unreachable { from: IpAddr::V6(from), code },
            None => Probe::Timeout,
        })
    }

    /// Sends IPv6 echo requests one after another.
    pub fn ping6(&mut self, t: &Topology, dev: crate::DeviceId, dst: Ipv6Addr, opts: PingOptions) -> Result<Vec<Probe>, SimError> {
        let kind = t.device(dev).ok_or(SimError::NoDevice(dev))?.kind;
        let hops = opts.ttl.unwrap_or(hop_limit(kind)).max(1);
        (1..=opts.count.clamp(1, 1_000)).map(|seq| self.probe6(t, dev, dst, hops, seq, opts.size, opts.timeout_us)).collect()
    }

    /// Probes with increasing hop limits until the destination answers.
    pub fn traceroute6(
        &mut self,
        t: &Topology,
        dev: crate::DeviceId,
        dst: Ipv6Addr,
        max_hops: u8,
        timeout_us: u64,
    ) -> Result<Vec<crate::Hop>, SimError> {
        let mut hops = Vec::new();
        for ttl in 1..=max_hops.clamp(1, 64) {
            let probes = (1..=3).map(|seq| self.probe6(t, dev, dst, ttl, seq, 32, timeout_us)).collect::<Result<Vec<_>, _>>()?;
            let done = probes.iter().any(|p| matches!(p, Probe::Reply { .. } | Probe::Unreachable { .. }));
            hops.push(crate::Hop { ttl, probes });
            if done {
                break;
            }
        }
        Ok(hops)
    }

    /// DHCPv6 messages: a server's answers to clients on this interface, or a client's replies.
    fn udp6_in(&mut self, t: &Topology, d: &Device, ifc: &L3If6, ip: Ipv6) -> Result<(), String> {
        let (_, dst_port, body) = dhcp6::udp6_decode(ip.src, ip.dst, &ip.payload).map_err(|e| e.to_string())?;
        if dst_port == dhcp6::SERVER_PORT
            && let Some(r) = dhcp6::Relay::decode(&body)
        {
            let r = r.map_err(|e| e.to_string())?;
            return if r.reply { self.dhcp6_relay_back(t, d, r) } else { self.dhcp6_serve_relayed(t, d, ifc, &ip, r) };
        }
        let msg = Dhcp6::decode(&body).map_err(|e| e.to_string())?;
        let conf = conf6(d, ifc).cloned().unwrap_or_default();
        // A relay agent passes clients' messages on to its servers.
        if dst_port == dhcp6::SERVER_PORT && conf.dhcp_server.is_none() && !conf.dhcp_relay.is_empty() {
            let link = ifc.addresses.first().map_or(Ipv6Addr::UNSPECIFIED, |n| n.addr);
            let fwd = dhcp6::Relay { reply: false, hops: 0, link, peer: ip.src, message: body };
            for server in conf.dhcp_relay {
                self.udp6_send(t, d, server, dhcp6::SERVER_PORT, dhcp6::SERVER_PORT, &fwd.encode())?;
            }
            return Ok(());
        }
        match dst_port {
            dhcp6::CLIENT_PORT if d.kind.is_host() => {
                let st = self.st(d.id);
                if st.dhcp6_inbox.len() < 16 {
                    st.dhcp6_inbox.push(msg);
                }
                Ok(())
            }
            dhcp6::SERVER_PORT => {
                let reply = self.dhcp6_serve(d, ifc, &msg, None)?;
                let src = ifc.link_local;
                let udp = dhcp6::udp6_encode(src, ip.src, dhcp6::SERVER_PORT, dhcp6::CLIENT_PORT, &reply.encode());
                let packet = Ipv6 { hop_limit: 64, next_header: netlabcraft_wire::PROTO_UDP, src, dst: ip.src, payload: udp };
                self.send_ip6(t, d, packet, Some(ifc))
            }
            p => Err(format!("UDP port {p} not open here")),
        }
    }

    /// A server's answer to a client message: from the pool whose prefix holds `link` (a
    /// relayed message), else the interface's `ipv6 dhcp server` pool.
    fn dhcp6_serve(&mut self, d: &Device, ifc: &L3If6, msg: &Dhcp6, link: Option<Ipv6Addr>) -> Result<Dhcp6, String> {
        let by_link = link.and_then(|l| d.dhcp6_pools.iter().find(|p| p.prefix.is_some_and(|n| n.contains(l))));
        let pool = match by_link {
            Some(p) => p.clone(),
            None => {
                let name = conf6(d, ifc).and_then(|c| c.dhcp_server.clone()).ok_or("no DHCPv6 server on this interface")?;
                d.dhcp6_pools.iter().find(|p| p.name == name).cloned().ok_or_else(|| format!("DHCPv6 pool {name} doesn't exist"))?
            }
        };
        let client = msg.client.ok_or("DHCPv6 message without a client id")?;
        let ia = match (msg.kind, msg.ia, pool.prefix) {
            (dhcp6::Kind::Solicit | dhcp6::Kind::Request, Some((iaid, _)), Some(prefix)) => {
                let addr = self.dhcp6_address(d, client, prefix, &pool.name, msg.kind == dhcp6::Kind::Request)?;
                Some((iaid, Some(addr)))
            }
            _ => None,
        };
        let kind = if msg.kind == dhcp6::Kind::Solicit { dhcp6::Kind::Advertise } else { dhcp6::Kind::Reply };
        Ok(Dhcp6 { kind, xid: msg.xid, client: Some(client), server: Some(ifc.mac), ia, dns: pool.dns.clone(), domain: pool.domain.clone() })
    }

    /// Sends a UDP datagram this device originates, from the address of the interface it leaves by.
    fn udp6_send(&mut self, t: &Topology, d: &Device, dst: Ipv6Addr, sport: u16, dport: u16, payload: &[u8]) -> Result<(), String> {
        let dynamic = self.dynamic_routes6(t, d);
        let (out, _) = route6(t, d, dst, None, &dynamic).ok_or_else(|| format!("no IPv6 route to {dst}"))?;
        let src = out.source_for(dst);
        let udp = dhcp6::udp6_encode(src, dst, sport, dport, payload);
        self.send_ip6(t, d, Ipv6 { hop_limit: 64, next_header: netlabcraft_wire::PROTO_UDP, src, dst, payload: udp }, None)
    }

    /// A server answering a relayed message: the answer goes back to the relay, wrapped.
    fn dhcp6_serve_relayed(&mut self, t: &Topology, d: &Device, ifc: &L3If6, ip: &Ipv6, r: dhcp6::Relay) -> Result<(), String> {
        let inner = Dhcp6::decode(&r.message).map_err(|e| e.to_string())?;
        let answer = self.dhcp6_serve(d, ifc, &inner, Some(r.link))?;
        let back = dhcp6::Relay { reply: true, hops: r.hops, link: r.link, peer: r.peer, message: answer.encode() };
        self.udp6_send(t, d, ip.src, dhcp6::SERVER_PORT, dhcp6::SERVER_PORT, &back.encode())
    }

    /// A relay agent passing the server's answer to the client it came for.
    fn dhcp6_relay_back(&mut self, t: &Topology, d: &Device, r: dhcp6::Relay) -> Result<(), String> {
        let ifs = up_ifaces6(t, d);
        let out = ifs
            .iter()
            .find(|i| i.addresses.iter().any(|a| a.addr == r.link))
            .or_else(|| ifs.iter().find(|i| conf6(d, i).is_some_and(|c| !c.dhcp_relay.is_empty())))
            .cloned()
            .ok_or("no interface for this relayed answer")?;
        let src = out.link_local;
        let udp = dhcp6::udp6_encode(src, r.peer, dhcp6::SERVER_PORT, dhcp6::CLIENT_PORT, &r.message);
        self.send_ip6(t, d, Ipv6 { hop_limit: 64, next_header: netlabcraft_wire::PROTO_UDP, src, dst: r.peer, payload: udp }, Some(&out))
    }

    /// The address a client gets from a pool: the one it already has, else the first free
    /// one from `prefix::10` up. `commit` records the binding.
    fn dhcp6_address(&mut self, d: &Device, client: MacAddr, prefix: Ipv6Net, pool: &str, commit: bool) -> Result<Ipv6Addr, String> {
        let st = self.st(d.id);
        if let Some((a, _)) = st.dhcp6_leases.iter().find(|(a, (m, _))| *m == client && prefix.contains(**a)) {
            return Ok(*a);
        }
        let base = u128::from(prefix.network().addr);
        let free =
            (0x10u128..0x10_000).map(|n| Ipv6Addr::from(base | n)).find(|a| !st.dhcp6_leases.contains_key(a)).ok_or("the DHCPv6 pool is full")?;
        if commit {
            st.dhcp6_leases.insert(free, (client, pool.to_string()));
        }
        Ok(free)
    }

    /// Sends a client DHCPv6 message from a host and waits for the answer of `want`.
    fn dhcp6_ask(&mut self, t: &Topology, d: &Device, ifc: &L3If6, msg: Dhcp6, want: dhcp6::Kind) -> Option<Dhcp6> {
        let dev = d.id;
        self.st(dev).dhcp6_inbox.clear();
        let xid = msg.xid;
        let udp = dhcp6::udp6_encode(ifc.link_local, dhcp6::ALL_SERVERS, dhcp6::CLIENT_PORT, dhcp6::SERVER_PORT, &msg.encode());
        let p = Ipv6 { hop_limit: 1, next_header: netlabcraft_wire::PROTO_UDP, src: ifc.link_local, dst: dhcp6::ALL_SERVERS, payload: udp };
        self.emit6(t, d, ifc, multicast_mac(dhcp6::ALL_SERVERS), p.encode());
        let start = self.now;
        self.advance(t, start.saturating_add(ND_WAIT_US), |s| {
            s.state.get(&dev).is_some_and(|st| st.dhcp6_inbox.iter().any(|m| m.xid == xid && m.kind == want))
        });
        let st = self.st(dev);
        st.dhcp6_inbox.iter().position(|m| m.xid == xid && m.kind == want).map(|i| st.dhcp6_inbox.remove(i))
    }

    /// Automatic configuration on a host, the way the router says: SLAAC, SLAAC plus stateless
    /// DHCPv6 (O flag), or stateful DHCPv6 (M flag).
    pub fn autoconfig6_full(&mut self, t: &Topology, dev: crate::DeviceId) -> Result<Auto6, SimError> {
        let (net, router) = self.autoconfig6(t, dev)?;
        let flags = self.last_ra_flags;
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let ifc = up_ifaces6(t, d).into_iter().next().ok_or_else(|| SimError::NoSource("no IPv6 interface".into()))?;
        self.next_echo_id = self.next_echo_id.wrapping_add(1);
        let xid = u32::from(self.next_echo_id) | 0x0001_0000;
        if flags & RA_MANAGED != 0 {
            let ask =
                Dhcp6 { kind: dhcp6::Kind::Solicit, xid, client: Some(ifc.mac), server: None, ia: Some((1, None)), dns: Vec::new(), domain: None };
            let adv =
                self.dhcp6_ask(t, d, &ifc, ask, dhcp6::Kind::Advertise).ok_or_else(|| SimError::NoSource("no DHCPv6 server answered".into()))?;
            let offered = adv.ia.and_then(|(_, a)| a).ok_or_else(|| SimError::NoSource("the DHCPv6 server has no addresses to give".into()))?;
            let req = Dhcp6 {
                kind: dhcp6::Kind::Request,
                xid: xid + 1,
                client: Some(ifc.mac),
                server: adv.server,
                ia: Some((1, Some(offered))),
                dns: Vec::new(),
                domain: None,
            };
            let rep =
                self.dhcp6_ask(t, d, &ifc, req, dhcp6::Kind::Reply).ok_or_else(|| SimError::NoSource("the DHCPv6 server didn't confirm".into()))?;
            let addr = rep.ia.and_then(|(_, a)| a).unwrap_or(offered);
            return Ok(Auto6 { address: Ipv6Net::new(addr, 64), router, dns: rep.dns.first().copied(), how: "stateful DHCPv6" });
        }
        if flags & RA_OTHER != 0 {
            let ask =
                Dhcp6 { kind: dhcp6::Kind::InformationRequest, xid, client: Some(ifc.mac), server: None, ia: None, dns: Vec::new(), domain: None };
            let rep = self.dhcp6_ask(t, d, &ifc, ask, dhcp6::Kind::Reply);
            return Ok(Auto6 { address: net, router, dns: rep.and_then(|r| r.dns.first().copied()), how: "SLAAC and stateless DHCPv6" });
        }
        Ok(Auto6 { address: net, router, dns: None, how: "SLAAC" })
    }

    /// Stateless autoconfiguration on a host: asks the routers on the link and builds an
    /// address from the first /64 prefix advertised. Returns the address and the router.
    pub fn autoconfig6(&mut self, t: &Topology, dev: crate::DeviceId) -> Result<(Ipv6Net, Ipv6Addr), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let ifc =
            up_ifaces6(t, d).into_iter().next().ok_or_else(|| SimError::NoSource(format!("{} has no cabled interface with IPv6 on", d.name)))?;
        self.st(dev).ra = None;
        let rs = Icmp6::RouterSolicit { mac: Some(ifc.mac) };
        let p = Ipv6 {
            hop_limit: 255,
            next_header: PROTO_ICMPV6,
            src: ifc.link_local,
            dst: ALL_ROUTERS,
            payload: rs.encode(ifc.link_local, ALL_ROUTERS),
        };
        self.emit6(t, d, &ifc, multicast_mac(ALL_ROUTERS), p.encode());
        let start = self.now;
        self.advance(t, start.saturating_add(ND_WAIT_US), |s| s.state.get(&dev).is_some_and(|st| st.ra.is_some()));
        let (router, prefixes, flags) =
            self.st(dev).ra.take().ok_or_else(|| SimError::NoSource("no IPv6 router answered (is `ipv6 unicast-routing` on?)".into()))?;
        self.last_ra_flags = flags;
        let p = prefixes.iter().find(|p| p.len == 64).ok_or_else(|| SimError::NoSource(format!("router {router} advertises no /64 prefix")))?;
        Ok((Ipv6Net::new(eui64(p.prefix, ifc.mac), 64), router))
    }
}
