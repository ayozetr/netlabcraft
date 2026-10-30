//! IPv6: interfaces, neighbour discovery, routing, echo and stateless autoconfiguration.
//!
//! Same shape as the IPv4 side: neighbour solicitations stand in for ARP (sent to the target's
//! solicited-node group), and every device holds packets until the neighbour answers. Routers with `ipv6 unicast-routing` answer router
//! solicitations with the /64 prefixes of the interface, and a host on autoconfiguration
//! builds its address from that prefix and its MAC (EUI-64).

use std::net::{IpAddr, Ipv6Addr};

use packetcraft_core::{Ipv6Net, MacAddr, eui64, link_local};
use packetcraft_model::{Device, DeviceKind, End, Topology};
use packetcraft_wire::Frame;
use packetcraft_wire::ipv6::{ALL_NODES, ALL_ROUTERS, ETHERTYPE_IPV6, Icmp6, Ipv6, PROTO_ICMPV6, PrefixInfo, multicast_mac, solicited_node};

use crate::{ARP_AGING_US, ArpEntry, IcmpKind, MAX_PENDING, PingOptions, Probe, Sim, SimError, port_up};

const ND_WAIT_US: u64 = 2_000_000;
const ND_RETRY_US: u64 = 1_000_000;
const MAX_ND_ENTRIES: usize = 4_096;

/// An IPv6 interface: a routed port or a host's port with IPv6 on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L3If6 {
    pub port: usize,
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

/// The IPv6 interfaces a device has on and not shut down.
pub fn l3_ifaces6(d: &Device) -> Vec<L3If6> {
    if matches!(d.kind, DeviceKind::Switch | DeviceKind::Hub) {
        return Vec::new();
    }
    d.ports
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.shutdown && p.ipv6.active())
        .map(|(i, p)| L3If6 {
            port: i,
            name: p.name.clone(),
            mac: p.mac,
            link_local: p.ipv6.link_local.unwrap_or_else(|| link_local(p.mac)),
            addresses: p.ipv6.addresses.clone(),
        })
        .collect()
}

/// The IPv6 interfaces that are up (cabled, both ends on).
pub fn up_ifaces6(t: &Topology, d: &Device) -> Vec<L3If6> {
    l3_ifaces6(d).into_iter().filter(|i| port_up(t, End { device: d.id, port: i.port })).collect()
}

/// One line of `show ipv6 route`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route6Entry {
    /// `C` connected, `L` local, `S` static.
    pub code: char,
    pub net: Ipv6Net,
    pub iface: Option<String>,
    pub next_hop: Option<Ipv6Addr>,
}

/// A device's IPv6 routing table: connected and local routes of its up interfaces, then the
/// static routes that can be used.
pub fn routing_table6(t: &Topology, d: &Device) -> Vec<Route6Entry> {
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
    out
}

/// Where a packet to `dst` leaves, and the neighbour it is handed to. `via` pins the interface
/// for link-local and multicast destinations (the one a request came in on).
pub fn route6(t: &Topology, d: &Device, dst: Ipv6Addr, via: Option<&L3If6>) -> Option<(L3If6, Ipv6Addr)> {
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
    let table = routing_table6(t, d);
    let best = table.iter().filter(|r| r.code != 'L' && r.net.contains(dst)).max_by_key(|r| (r.net.prefix, r.code == 'C'))?;
    match (best.code, &best.iface, best.next_hop) {
        ('C', Some(name), _) => ifs.iter().find(|i| i.name == *name).map(|i| (i.clone(), dst)),
        (_, Some(name), Some(nh)) => ifs.iter().find(|i| i.name == *name).map(|i| (i.clone(), nh)),
        (_, Some(name), None) => ifs.iter().find(|i| i.name == *name).map(|i| (i.clone(), dst)),
        (_, None, Some(nh)) => ifs.iter().find(|i| i.addresses.iter().any(|a| a.contains(nh))).map(|i| (i.clone(), nh)),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Pending6 {
    next_hop: Ipv6Addr,
    port: usize,
    packet: Vec<u8>,
    since: u64,
}

impl Pending6 {
    pub(crate) fn port(&self) -> usize {
        self.port
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
        if f.vlan.is_some() {
            return Err("IPv6 on subinterfaces is not simulated yet".into());
        }
        let ifc = l3_ifaces6(d).into_iter().find(|i| i.port == in_port).ok_or("IPv6 is not enabled on this interface")?;
        let ip = Ipv6::decode(&f.payload).map_err(|e| e.to_string())?;
        if let Err(e) = self.acl6_check(d, ifc.port, &ip, true) {
            self.acl6_refuse(t, d, &ifc, &ip);
            return Err(e);
        }
        let router = d.kind == DeviceKind::Router && d.ipv6_routing;
        let all = l3_ifaces6(d);
        let joined = |g: Ipv6Addr| {
            g == ALL_NODES
                || (router && g == ALL_ROUTERS)
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
        match route6(t, d, ip.dst, None) {
            Some((out, nh)) => {
                if let Err(e) = self.acl6_check(d, out.port, &ip, false) {
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
                    Icmp6::Unreachable { code: packetcraft_wire::ipv6::UNREACH6_NO_ROUTE, original: Icmp6::quote(&raw) },
                );
                Err(format!("no IPv6 route to {}", ip.dst))
            }
        }
    }

    fn local6_in(&mut self, t: &Topology, d: &Device, ifc: &L3If6, src_mac: MacAddr, ip: Ipv6) -> Result<(), String> {
        if ip.next_header != PROTO_ICMPV6 {
            return Err(format!("IPv6 next header {} not understood", ip.next_header));
        }
        let msg = Icmp6::decode(&ip.payload, ip.src, ip.dst).map_err(|e| e.to_string())?;
        let now = self.now;
        let router = d.kind == DeviceKind::Router && d.ipv6_routing;
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
                let na =
                    Icmp6::NeighborAdvert { target, router: d.kind == DeviceKind::Router, solicited: !ip.src.is_unspecified(), mac: Some(ifc.mac) };
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
                let ra = Icmp6::RouterAdvert { hop_limit: 64, lifetime: 1_800, mac: Some(ifc.mac), prefixes };
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
            Icmp6::RouterAdvert { lifetime, mac, prefixes, .. } => {
                if d.kind.is_host() && lifetime > 0 {
                    self.learn_nd(t, d, ifc, ip.src, mac.unwrap_or(src_mac));
                    self.st(d.id).ra = Some((ip.src, prefixes));
                }
                Ok(())
            }
        }
    }

    /// Applies the IPv6 access list on a port, if one is set and exists.
    fn acl6_check(&mut self, d: &Device, port: usize, ip: &Ipv6, inbound: bool) -> Result<(), String> {
        let Some(p) = d.ports.get(port) else { return Ok(()) };
        let Some(name) = (if inbound { &p.ipv6.filter_in } else { &p.ipv6.filter_out }) else { return Ok(()) };
        // A filter naming a list that doesn't exist lets everything through.
        let Some(acl) = d.acls6.iter().find(|a| a.name == *name) else { return Ok(()) };
        let nd = ip.next_header == PROTO_ICMPV6 && ip.payload.first().is_some_and(|t| (133..=137).contains(t));
        let key = packetcraft_model::acl::Packet6Key { next_header: ip.next_header, src: ip.src, dst: ip.dst, src_port: None, dst_port: None, nd };
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
            std::mem::take(&mut st.pending6).into_iter().partition(|p| p.next_hop == ip && p.port == ifc.port);
        st.pending6 = rest;
        for p in ready {
            self.emit(
                t,
                End { device: d.id, port: p.port },
                Frame { dst: mac, src: ifc.mac, vlan: None, ethertype: ETHERTYPE_IPV6, payload: p.packet }.encode(),
            );
        }
    }

    fn emit6(&mut self, t: &Topology, d: &Device, ifc: &L3If6, to: MacAddr, packet: Vec<u8>) {
        let frame = Frame { dst: to, src: ifc.mac, vlan: None, ethertype: ETHERTYPE_IPV6, payload: packet };
        self.emit_frame(t, d, ifc.port, &frame);
    }

    /// Routes and sends a packet this device originates.
    fn send_ip6(&mut self, t: &Topology, d: &Device, ip: Ipv6, via: Option<&L3If6>) -> Result<(), String> {
        let (out, nh) = route6(t, d, ip.dst, via).ok_or_else(|| format!("no IPv6 route to {}", ip.dst))?;
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
            st.pending6.push(Pending6 { next_hop: nh, port: out.port, packet, since: now });
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
        let Some((out, _)) = route6(t, d, dst, None) else {
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
        let (router, prefixes) =
            self.st(dev).ra.take().ok_or_else(|| SimError::NoSource("no IPv6 router answered (is `ipv6 unicast-routing` on?)".into()))?;
        let p = prefixes.iter().find(|p| p.len == 64).ok_or_else(|| SimError::NoSource(format!("router {router} advertises no /64 prefix")))?;
        Ok((Ipv6Net::new(eui64(p.prefix, ifc.mac), 64), router))
    }
}
