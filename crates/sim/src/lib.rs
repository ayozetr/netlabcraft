//! PacketCraft L3: the network simulator.
//!
//! A discrete-event simulator over simulated time (microseconds). Devices exchange real
//! Ethernet frames ([`packetcraft_wire`]) across the cables of a [`Topology`]; every delivery is
//! recorded in the event list ([`TraceEvent`]) so the UI can replay it and export a pcap.
//!
//! The simulator reads configuration from the topology on every event and keeps only dynamic
//! state of its own (ARP caches, MAC tables, packets waiting for ARP). After the topology
//! changes, call [`Sim::topology_changed`].
//!
//! Behaviour follows the public standards (IEEE 802.1D learning bridges, 802.1Q, RFC 826, RFC
//! 791, RFC 792, RFC 1812 for router forwarding).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};

use packetcraft_core::{DeviceId, Ipv4Net, MacAddr, VlanId};
use packetcraft_model::{Device, DeviceKind, End, NextHop, SwitchportMode, Topology, ifname};
use packetcraft_wire::dhcp::{self, Dhcp, DhcpKind};
use packetcraft_wire::tcp::{self, Tcp};
use packetcraft_wire::{Arp, ArpOp, ETHERTYPE_ARP, ETHERTYPE_IPV4, Frame, Icmp, Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP, Protocol, Udp, dns};

pub mod cdp;
pub mod email;
pub mod ftp;
pub mod lldp;

pub mod channel;
pub mod control;
pub mod eigrp;
pub mod firewall;
pub mod hsrp;
pub mod mgmt;
pub mod ospf;
pub mod ospf6;
pub mod qos;

pub mod rip;
pub mod snmp;
pub mod stp;
#[cfg(test)]
mod tests;
pub mod v6;
pub mod vpn;
pub mod vtp;
pub mod wlan;

/// One cable hop (serialisation + propagation), in microseconds.
pub const LINK_DELAY_US: u64 = 250;
/// Events one call may process before the simulator declares a storm and drops everything in
/// flight (a last-resort guard; loops are normally caught by [`MAX_SWITCH_HOPS`]).
pub const MAX_EVENTS_PER_RUN: usize = 200_000;
/// Switches one frame may cross. Ethernet has no hop limit, so without a spanning tree a frame
/// in a switching loop circles forever; past this many switches the simulator drops it and
/// reports a storm. Real switched networks are far shallower.
pub const MAX_SWITCH_HOPS: u8 = 64;
/// Event-list length; older events are forgotten.
pub const TRACE_CAP: usize = 20_000;
/// Where an unset device clock starts: 2026-01-01 00:00:00 UTC.
pub const DEFAULT_CLOCK: i64 = 1_767_225_600;
/// Most messages a logging buffer keeps.
pub const MAX_LOG: usize = 256;
pub const MAC_AGING_US: u64 = 300 * 1_000_000;
pub const ARP_AGING_US: u64 = 4 * 3_600 * 1_000_000;
/// How long a host keeps a packet waiting for an ARP reply, and how often it re-asks.
const ARP_WAIT_US: u64 = 2_000_000;
const ARP_RETRY_US: u64 = 1_000_000;
const MAX_PENDING: usize = 64;
const MAX_MAC_ENTRIES: usize = 8_192;
const MAX_ARP_ENTRIES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimError {
    #[error("no device {0}")]
    NoDevice(DeviceId),
    #[error("{0}")]
    NoSource(String),
}

/// A layer-3 interface: a routed port, a subinterface, or a host's port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L3If {
    pub port: usize,
    /// Subinterface number (`.10`), if any.
    pub sub: Option<u32>,
    /// The 802.1Q tag the interface sends and accepts (subinterfaces).
    pub vlan: Option<u16>,
    pub net: Ipv4Net,
    pub mac: MacAddr,
    pub name: String,
}

/// The layer-3 interfaces a device has configured and not shut down (whether or not cabled).
/// The port index that stands for a switch's VLAN interfaces (its own "CPU" port).
pub const SVI_PORT: usize = usize::MAX;

/// The MAC address of a switch's VLAN interfaces.
pub fn svi_mac(d: &Device) -> MacAddr {
    MacAddr::local(d.id, 0xfffe)
}

pub fn l3_ifaces(d: &Device) -> Vec<L3If> {
    if d.kind == DeviceKind::Switch {
        let mac = svi_mac(d);
        let mut out: Vec<L3If> = d
            .svis
            .iter()
            .filter(|s| !s.shutdown)
            .filter_map(|s| s.ip.map(|net| L3If { port: SVI_PORT, sub: None, vlan: Some(s.vlan.get()), net, mac, name: format!("Vlan{}", s.vlan) }))
            .collect();
        // Routed ports of a multilayer switch.
        for (i, p) in d.ports.iter().enumerate() {
            if let (true, false, Some(net)) = (p.routed, p.shutdown, p.ip) {
                out.push(L3If { port: i, sub: None, vlan: None, net, mac: p.mac, name: p.name.clone() });
            }
        }
        return out;
    }
    if d.kind == DeviceKind::Hub {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, p) in d.ports.iter().enumerate() {
        if p.shutdown {
            continue;
        }
        if let Some(net) = p.ip {
            out.push(L3If { port: i, sub: None, vlan: None, net, mac: p.mac, name: p.name.clone() });
        }
        if d.kind == DeviceKind::Router {
            for s in &p.subifs {
                if let (Some(net), Some(v), false) = (s.ip, s.vlan, s.shutdown) {
                    out.push(L3If { port: i, sub: Some(s.number), vlan: Some(v.get()), net, mac: p.mac, name: format!("{}.{}", p.name, s.number) });
                }
            }
        }
    }
    // GRE tunnels (routers).
    out.extend(vpn::tunnel_ifaces(d));
    out
}

/// Whether a port can pass traffic: not shut down, cabled, and the far port not shut down;
/// on a serial cable also clocked by its DCE end and framed alike at both ends.
pub fn port_up(t: &Topology, end: End) -> bool {
    let ok = |p: &packetcraft_model::Port| !p.shutdown && !p.err_disabled;
    let own = t.device(end.device).and_then(|d| d.ports.get(end.port)).is_some_and(ok);
    // An access point's radio is up while some host is associated with it.
    if is_ap_radio(t, end) {
        return own && !t.wifi_clients(end).is_empty();
    }
    own && t.peer(end).and_then(|p| t.device(p.device).and_then(|d| d.ports.get(p.port))).is_some_and(ok)
        && serial_problem(t, end).is_none()
        && !speed_mismatch(t, end)
        && cable_problem(t, end).is_none()
}

/// Why the copper cable at a port carries nothing, if it doesn't: longer than Ethernet over
/// twisted pair reaches. Serial cables stand for leased lines and have no limit here.
pub fn cable_problem(t: &Topology, end: End) -> Option<String> {
    if t.is_serial(end) {
        return None;
    }
    let l = t.link_at(end)?;
    let len = t.cable_length(l);
    let max = packetcraft_model::physical::COPPER_MAX_M;
    (len.meters > max).then(|| format!("cable too long: {} m copper, max {max} m", len.meters))
}

/// The fastest an Ethernet port goes, in Mbit/s, from its type (`None`: not Ethernet).
pub fn port_max_speed(name: &str) -> Option<u32> {
    match packetcraft_model::ifname::parse(name)?.kind {
        "GigabitEthernet" => Some(1000),
        "FastEthernet" | "Ethernet" | "Internet" => Some(100),
        _ => None,
    }
}

/// Both ends set to different fixed speeds: no link.
pub fn speed_mismatch(t: &Topology, end: End) -> bool {
    let port = |e: End| t.device(e.device).and_then(|d| d.ports.get(e.port));
    match (port(end).and_then(|p| p.speed), t.peer(end).and_then(port).and_then(|p| p.speed)) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}

/// What an Ethernet link runs at: the fixed speed of either end, else the fastest both ends
/// manage; full duplex unless this end is set to half. `None` when the link is down or not
/// Ethernet.
pub fn link_speed(t: &Topology, end: End) -> Option<(u32, packetcraft_model::Duplex)> {
    let port = |e: End| t.device(e.device).and_then(|d| d.ports.get(e.port));
    let p = port(end)?;
    let q = port(t.peer(end)?)?;
    if !port_up(t, end) {
        return None;
    }
    let speed = p.speed.or(q.speed).unwrap_or(port_max_speed(&p.name)?.min(port_max_speed(&q.name)?));
    Some((speed, p.duplex.unwrap_or(packetcraft_model::Duplex::Full)))
}

/// Whether a port is an access point's radio.
pub fn is_ap_radio(t: &Topology, end: End) -> bool {
    t.device(end.device).filter(|d| d.kind == DeviceKind::Switch).and_then(|d| d.ports.get(end.port)).is_some_and(|p| ifname::is_wireless(&p.name))
}

/// Why a cabled serial link can't carry traffic (its line protocol stays down), if it can't.
pub fn serial_problem(t: &Topology, end: End) -> Option<&'static str> {
    if !t.is_serial(end) {
        return None;
    }
    let port = |e: End| t.device(e.device).and_then(|d| d.ports.get(e.port));
    let peer = t.peer(end)?;
    let dce = t.dce_of(end)?;
    if port(dce).is_none_or(|p| p.serial.clock_rate.is_none()) {
        return Some("no clock rate on the DCE end");
    }
    if port(end).map(|p| p.serial.encapsulation) != port(peer).map(|p| p.serial.encapsulation) {
        return Some("the two ends use different encapsulations");
    }
    if port(end).is_some_and(|p| p.serial.encapsulation == packetcraft_model::Encapsulation::Ppp) && !ppp_authenticates(t, end, peer) {
        return Some("PPP authentication failed (check the usernames and passwords)");
    }
    None
}

/// Whether each end of a PPP link that demands authentication gets it from the other.
fn ppp_authenticates(t: &Topology, end: End, peer: End) -> bool {
    use packetcraft_model::PppAuth;
    [(end, peer), (peer, end)].into_iter().all(|(me, other)| {
        let (Some(dm), Some(dother)) = (t.device(me.device), t.device(other.device)) else { return false };
        let (Some(pm), Some(po)) = (dm.ports.get(me.port), dother.ports.get(other.port)) else { return false };
        if pm.serial.ppp_auth.is_empty() {
            return true;
        }
        let user = |d: &Device, name: &str| d.access.users.iter().find(|u| u.name == name).cloned();
        // Any method this end offers that the other end can satisfy.
        pm.serial.ppp_auth.iter().any(|m| match m {
            // Both ends name each other with the same secret.
            PppAuth::Chap => match (user(dm, &dother.name), user(dother, &dm.name)) {
                (Some(a), Some(b)) => a.same_secret(&b),
                _ => false,
            },
            PppAuth::Pap => po.serial.pap_sent.as_ref().is_some_and(|(u, pw)| user(dm, u).is_some_and(|x| x.matches(pw))),
        })
    })
}

/// The framing a serial port sends.
fn framing(p: &packetcraft_model::Port) -> packetcraft_wire::serial::Framing {
    match p.serial.encapsulation {
        packetcraft_model::Encapsulation::Hdlc => packetcraft_wire::serial::Framing::Hdlc,
        packetcraft_model::Encapsulation::Ppp => packetcraft_wire::serial::Framing::Ppp,
    }
}

/// Something the simulator decided that changes the lab: the engine applies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortEvent {
    /// A port-security violation shut the port down.
    ErrDisabled { device: DeviceId, port: usize },
    /// A sticky address was learned.
    Sticky { device: DeviceId, port: usize, mac: MacAddr },
}

/// The layer-3 interfaces that are up (connected routes exist only for these).
pub fn up_ifaces(t: &Topology, d: &Device) -> Vec<L3If> {
    let mut out = phys_up_ifaces(t, d);
    if !d.tunnels.is_empty() {
        let tunnels = vpn::up_tunnels(t, d, &out);
        out.extend(tunnels);
    }
    out
}

/// [`up_ifaces`] without tunnels: ports, subinterfaces and VLAN interfaces.
pub(crate) fn phys_up_ifaces(t: &Topology, d: &Device) -> Vec<L3If> {
    l3_ifaces(d)
        .into_iter()
        .filter(|i| !vpn::is_tunnel(i.port))
        .filter(|i| {
            if i.port == SVI_PORT {
                // A VLAN interface is up while some port of its VLAN is up.
                i.vlan
                    .and_then(VlanId::new)
                    .is_some_and(|v| (0..d.ports.len()).any(|p| stp::carries(t, d, p, v) && port_up(t, End { device: d.id, port: p })))
            } else {
                port_up(t, End { device: d.id, port: i.port })
            }
        })
        .collect()
}

/// A routing-table row, as `show ip route` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEntry {
    /// `C` connected, `L` local address, `S` static.
    pub code: char,
    pub net: Ipv4Net,
    pub via: Option<Ipv4Addr>,
    pub iface: Option<String>,
}

/// The device's routing table: connected and local routes of up interfaces, static routes
/// whose next hop resolves, then what the routing protocols learned. For a prefix several
/// protocols know, the lowest administrative distance wins: EIGRP (`D`, 90), OSPF (`O`, 110;
/// `I` inter-area, `E` external default), RIP (`R`, 120); connected and static routes beat them all.
pub fn routing_table(t: &Topology, d: &Device) -> Vec<RouteEntry> {
    let e = if d.eigrp.is_some() { eigrp::compute(t).routes.remove(&d.id).unwrap_or_default() } else { Vec::new() };
    let o = if d.ospf.is_some() { ospf::compute(t).routes.remove(&d.id).unwrap_or_default() } else { Vec::new() };
    let r = if d.rip.is_some() { rip::compute(t).routes.remove(&d.id).unwrap_or_default() } else { Vec::new() };
    routing_table_with(t, d, &e, &o, &r)
}

/// [`routing_table`] with the routing protocols' routes for this device given. A destination
/// reached over several paths (load balancing) has a row per path.
pub fn routing_table_with(
    t: &Topology,
    d: &Device,
    eigrp_routes: &[eigrp::EigrpRoute],
    ospf_routes: &[ospf::OspfRoute],
    rip_routes: &[rip::RipRoute],
) -> Vec<RouteEntry> {
    let mut out = routing_table_static(t, d);
    let mut learned: Vec<(u8, RouteEntry)> = Vec::new();
    if d.eigrp.is_some() {
        for r in eigrp_routes {
            learned.push((eigrp::DISTANCE, RouteEntry { code: 'D', net: r.net, via: Some(r.via), iface: Some(r.iface.clone()) }));
        }
    }
    if d.ospf.is_some() {
        for r in ospf_routes {
            let code = if r.external {
                'E'
            } else if r.inter_area {
                'I'
            } else {
                'O'
            };
            learned.push((110, RouteEntry { code, net: r.net, via: Some(r.via), iface: Some(r.iface.clone()) }));
        }
    }
    if d.rip.is_some() {
        for r in rip_routes {
            learned.push((120, RouteEntry { code: 'R', net: r.net, via: Some(r.via), iface: Some(r.iface.clone()) }));
        }
    }
    let have: Vec<Ipv4Net> = out.iter().map(|r| r.net).collect();
    for (ad, r) in &learned {
        if !have.contains(&r.net) && !learned.iter().any(|(other, x)| x.net == r.net && other < ad) {
            out.push(r.clone());
        }
    }
    // Stable: the paths of one destination keep the order the protocol gave them.
    out.sort_by_key(|r| (r.net.addr, r.net.prefix, r.code));
    out
}

fn routing_table_static(t: &Topology, d: &Device) -> Vec<RouteEntry> {
    let ifs = up_ifaces(t, d);
    let mut out = Vec::new();
    for i in &ifs {
        out.push(RouteEntry { code: 'C', net: i.net.network(), via: None, iface: Some(i.name.clone()) });
        if i.net.prefix < 32 {
            out.push(RouteEntry { code: 'L', net: Ipv4Net::new(i.net.addr, 32), via: None, iface: Some(i.name.clone()) });
        }
    }
    if d.routes_ip() && !d.routes.iter().any(|r| r.net.prefix == 0) {
        for (i, p) in d.ports.iter().enumerate() {
            if let Some(gw) = p.dhcp_router
                && ifs.iter().any(|x| x.port == i && x.net.contains(gw))
            {
                out.push(RouteEntry { code: 'S', net: Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0), via: Some(gw), iface: None });
            }
        }
    }
    if d.routes_ip() {
        for r in &d.routes {
            match &r.via {
                NextHop::Address(a) => {
                    if ifs.iter().any(|i| i.net.contains(*a)) {
                        out.push(RouteEntry { code: 'S', net: r.net, via: Some(*a), iface: None });
                    }
                }
                NextHop::Interface(n) => {
                    if let Some(i) = find_iface(&ifs, n) {
                        out.push(RouteEntry { code: 'S', net: r.net, via: None, iface: Some(i.name.clone()) });
                    }
                }
            }
        }
    }
    out.sort_by_key(|r| (r.net.addr, r.net.prefix, r.code));
    out
}

fn find_iface<'a>(ifs: &'a [L3If], name: &str) -> Option<&'a L3If> {
    if packetcraft_model::vpn::tunnel_number(name).is_some() {
        return ifs.iter().find(|i| vpn::same_name(&i.name, name));
    }
    let n = ifname::parse(name)?;
    ifs.iter().find(|i| ifname::parse(&i.name).is_some_and(|m| m == n))
}

/// Where a datagram to `dst` leaves a device: the interface and the next hop to ARP for.
pub fn route(t: &Topology, d: &Device, dst: Ipv4Addr) -> Option<(L3If, Ipv4Addr)> {
    let st = if d.ospf.is_some() { Some(ospf::compute(t)) } else { None };
    let rp = if d.rip.is_some() { Some(rip::compute(t)) } else { None };
    let ep = if d.eigrp.is_some() { Some(eigrp::compute(t)) } else { None };
    route_with(
        t,
        d,
        dst,
        flow_hash(Ipv4Addr::UNSPECIFIED, dst),
        ep.as_ref().and_then(|s| s.routes.get(&d.id)).map_or(&[][..], Vec::as_slice),
        st.as_ref().and_then(|s| s.routes.get(&d.id)).map_or(&[][..], Vec::as_slice),
        rp.as_ref().and_then(|s| s.routes.get(&d.id)).map_or(&[][..], Vec::as_slice),
    )
}

/// A flow's hash, for choosing among equal paths: the same source and destination always take
/// the same path.
pub fn flow_hash(src: Ipv4Addr, dst: Ipv4Addr) -> u32 {
    // FNV-1a over both addresses.
    let mut h: u32 = 0x811c_9dc5;
    for b in src.octets().into_iter().chain(dst.octets()) {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^ (h >> 16)
}

/// Picks one of several paths to the same destination for a flow, each path weighted (traffic
/// share): the paths are `(weight, item)` in a fixed order.
fn pick_path<T: Clone>(paths: &[(u32, T)], flow: u32) -> Option<T> {
    let total: u64 = paths.iter().map(|(w, _)| u64::from((*w).max(1))).sum();
    if total == 0 {
        return None;
    }
    let mut at = u64::from(flow) % total;
    for (w, p) in paths {
        let w = u64::from((*w).max(1));
        if at < w {
            return Some(p.clone());
        }
        at -= w;
    }
    paths.last().map(|(_, p)| p.clone())
}

/// [`route`] with the routing protocols' routes for this device given (at equal prefix length
/// connected beats static beats EIGRP beats OSPF beats RIP, as their administrative distances say).
/// Among several paths to the most specific destination, `flow` picks one.
#[allow(clippy::too_many_arguments)]
pub fn route_with(
    t: &Topology,
    d: &Device,
    dst: Ipv4Addr,
    flow: u32,
    eigrp_routes: &[eigrp::EigrpRoute],
    ospf_routes: &[ospf::OspfRoute],
    rip_routes: &[rip::RipRoute],
) -> Option<(L3If, Ipv4Addr)> {
    let ifs = up_ifaces(t, d);
    if d.kind.is_host() {
        let i = ifs.into_iter().next()?;
        if i.net.contains(dst) || dst == Ipv4Addr::BROADCAST {
            return Some((i, dst));
        }
        let gw = d.gateway.filter(|g| i.net.contains(*g))?;
        return Some((i, gw));
    }
    if d.kind == DeviceKind::Switch && !d.ip_routing {
        // A switch doesn't route: connected VLAN interfaces, else its default gateway.
        if let Some(i) = ifs.iter().filter(|i| i.net.contains(dst)).max_by_key(|i| i.net.prefix) {
            return Some((i.clone(), dst));
        }
        let gw = d.gateway?;
        let i = ifs.into_iter().find(|i| i.net.contains(gw))?;
        return Some((i, gw));
    }
    if d.kind == DeviceKind::Hub {
        return None;
    }
    let mut best: Option<(u8, L3If, Ipv4Addr)> = None;
    let mut consider = |prefix: u8, i: &L3If, nh: Ipv4Addr| {
        if best.as_ref().is_none_or(|(p, _, _)| prefix > *p) {
            best = Some((prefix, i.clone(), nh));
        }
    };
    for i in &ifs {
        if i.net.contains(dst) {
            consider(i.net.prefix, i, dst);
        }
    }
    for r in &d.routes {
        if !r.net.contains(dst) {
            continue;
        }
        match &r.via {
            NextHop::Address(a) => {
                if let Some(i) = ifs.iter().filter(|i| i.net.contains(*a)).max_by_key(|i| i.net.prefix) {
                    consider(r.net.prefix, i, *a);
                }
            }
            NextHop::Interface(n) => {
                if let Some(i) = find_iface(&ifs, n) {
                    consider(r.net.prefix, i, dst);
                }
            }
        }
    }
    // EIGRP before OSPF: at equal prefix length the first one considered wins (distance 90).
    // Several paths to the same destination share the traffic, flow by flow: EIGRP's in
    // proportion to their metrics (unequal-cost with `variance`), OSPF's equally.
    let mut nets: Vec<Ipv4Net> = eigrp_routes.iter().filter(|r| r.net.contains(dst)).map(|r| r.net).collect();
    nets.dedup();
    for net in nets {
        let paths: Vec<&eigrp::EigrpRoute> = eigrp_routes.iter().filter(|r| r.net == net).collect();
        let worst = paths.iter().map(|r| r.metric).max().unwrap_or(1).max(1);
        let usable: Vec<(u32, (L3If, Ipv4Addr))> =
            paths.iter().filter_map(|r| ifs.iter().find(|i| i.name == r.iface).map(|i| (worst / r.metric.max(1), (i.clone(), r.via)))).collect();
        if let Some((i, nh)) = pick_path(&usable, flow) {
            consider(net.prefix, &i, nh);
        }
    }
    let mut nets: Vec<Ipv4Net> = ospf_routes.iter().filter(|r| r.net.contains(dst)).map(|r| r.net).collect();
    nets.dedup();
    for net in nets {
        let usable: Vec<(u32, (L3If, Ipv4Addr))> = ospf_routes
            .iter()
            .filter(|r| r.net == net)
            .filter_map(|r| ifs.iter().find(|i| i.name == r.iface).map(|i| (1, (i.clone(), r.via))))
            .collect();
        if let Some((i, nh)) = pick_path(&usable, flow) {
            consider(net.prefix, &i, nh);
        }
    }
    // RIP last: at equal prefix length the others win (administrative distance 120).
    for r in rip_routes.iter().filter(|r| r.net.contains(dst)) {
        if let Some(i) = ifs.iter().find(|i| i.name == r.iface) {
            consider(r.net.prefix, i, r.via);
        }
    }
    // A default from a DHCP lease (distance 254): considered last, so any other route to the
    // same length wins.
    for (i, p) in d.ports.iter().enumerate() {
        if let Some(gw) = p.dhcp_router
            && let Some(ifc) = ifs.iter().find(|x| x.port == i && x.net.contains(gw))
        {
            consider(0, ifc, gw);
        }
    }
    best.map(|(_, i, nh)| (i, nh))
}

/// A learned ARP binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArpEntry {
    pub mac: MacAddr,
    pub learned_us: u64,
    /// The interface it was learned on.
    pub iface: String,
}

/// A learned MAC-table entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacEntry {
    pub port: usize,
    pub learned_us: u64,
}

#[derive(Debug, Clone)]
struct Pending {
    next_hop: Ipv4Addr,
    iface: L3If,
    datagram: Vec<u8>,
    since: u64,
}

/// What a `debug` command reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugKind {
    Icmp,
    Rip,
    Ospf,
    Eigrp,
    Packet,
    Nat,
    Dhcp,
}

impl DebugKind {
    pub const ALL: [DebugKind; 7] =
        [DebugKind::Icmp, DebugKind::Rip, DebugKind::Ospf, DebugKind::Eigrp, DebugKind::Packet, DebugKind::Nat, DebugKind::Dhcp];
}

/// What came back for one of our echo requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcmpKind {
    Reply,
    TimeExceeded,
    Unreachable(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IcmpEvent {
    at: u64,
    from: Ipv4Addr,
    id: u16,
    seq: u16,
    kind: IcmpKind,
}

/// An address a DHCP server handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub mac: MacAddr,
    pub pool: String,
    pub since_us: u64,
}

/// What a DHCP client obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DhcpResult {
    pub address: Ipv4Net,
    pub router: Option<Ipv4Addr>,
    pub dns: Option<Ipv4Addr>,
    pub server: Option<Ipv4Addr>,
}

/// One NAT translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatEntry {
    /// IPv4 protocol number (1 ICMP, 17 UDP).
    pub protocol: u8,
    pub inside_local: Ipv4Addr,
    /// ICMP identifier or UDP port, when translated per flow (PAT).
    pub local_id: Option<u16>,
    pub inside_global: Ipv4Addr,
    pub global_id: Option<u16>,
    pub outside: Ipv4Addr,
    /// From an `ip nat inside source static` line.
    pub fixed: bool,
}

const MAX_NAT_ENTRIES: usize = 4_096;

/// Addresses a pool may scan for a free one.
const MAX_POOL_SCAN: u32 = 65_536;

/// A device's dynamic state.
#[derive(Debug, Clone, Default)]
pub struct DevState {
    pub arp: BTreeMap<Ipv4Addr, ArpEntry>,
    /// Keyed by (VLAN, MAC).
    pub macs: BTreeMap<(u16, MacAddr), MacEntry>,
    /// DHCP servers: addresses handed out.
    pub leases: BTreeMap<Ipv4Addr, Lease>,
    /// Access-list matches: (list, entry index or `usize::MAX` for the implicit deny) → count.
    pub acl_hits: BTreeMap<(String, usize), u64>,
    /// NAT translations in use.
    pub nat: Vec<NatEntry>,
    /// Port security: addresses learned per port (not sticky), and violations counted.
    pub secure: BTreeMap<usize, Vec<MacAddr>>,
    pub violations: BTreeMap<usize, u64>,
    dhcp_xid: Option<u32>,
    /// The port a router's DHCP client is asking on.
    dhcp_port: Option<usize>,
    dhcp_inbox: Vec<Dhcp>,
    /// A DNS question in flight: (local port, id), and its answer.
    dns_wait: Option<(u16, u16)>,
    dns_answer: Option<dns::Dns>,
    /// TCP segments for a client connection in flight: (local port, remote port).
    /// Connections this device is a client of: (local port, remote port).
    tcp_wait: Vec<(u16, u16)>,
    tcp_inbox: Vec<tcp::Tcp>,
    pending: Vec<Pending>,
    arp_asked: BTreeMap<Ipv4Addr, u64>,
    icmp: Vec<IcmpEvent>,
    /// IPv6 neighbour cache (the ARP table's counterpart).
    pub nd: BTreeMap<std::net::Ipv6Addr, ArpEntry>,
    nd_asked: BTreeMap<std::net::Ipv6Addr, u64>,
    pending6: Vec<v6::Pending6>,
    icmp6: Vec<v6::Icmp6Event>,
    /// `clock set`: Unix seconds at simulated time zero, when someone set the clock.
    clock_base: Option<i64>,
    /// The logging buffer (`show logging`): (simulated µs, message).
    pub log: VecDeque<(u64, String)>,
    /// Messages waiting to be printed on the device's console.
    console: Vec<String>,
    /// `debug ip icmp`.
    pub debug_icmp: bool,
    /// `debug ip rip`.
    pub debug_rip: bool,
    /// `debug ip ospf adj|events|hello`.
    pub debug_ospf: bool,
    /// `debug eigrp packets`.
    pub debug_eigrp: bool,
    /// `debug ip packet`.
    pub debug_packet: bool,
    /// `debug ip nat`.
    pub debug_nat: bool,
    /// `debug ip dhcp server events`.
    pub debug_dhcp: bool,
    /// Log lines waiting to go to syslog servers.
    syslog_out: Vec<String>,
    /// Syslog servers: messages received (µs, sender, text).
    pub syslog: VecDeque<(u64, Ipv4Addr, String)>,
    /// NTP: the server that last set this device's clock.
    pub ntp_synced: Option<Ipv4Addr>,
    /// TFTP client: the local port of the transfer in flight, and what came back.
    tftp_port: Option<u16>,
    tftp_inbox: Vec<packetcraft_wire::mgmt::Tftp>,
    /// A RADIUS client waiting on this port, and the answers that came.
    radius_port: Option<u16>,
    /// Failed remote logins (simulated µs), for `login block-for`, and when the block ends.
    login_failures: Vec<u64>,
    login_blocked_until: u64,
    radius_inbox: Vec<packetcraft_wire::radius::Radius>,
    /// An SNMP manager waiting on this port, and the answers that came.
    snmp_port: Option<u16>,
    snmp_inbox: Vec<Vec<u8>>,
    /// SNMP agents: notifications waiting to go to the trap hosts, and how many went.
    pub(crate) trap_out: Vec<snmp::TrapEvent>,
    pub traps_sent: u32,
    /// Hosts: SNMP traps received (newest last).
    pub traps: VecDeque<snmp::ReceivedTrap>,
    /// FTP server connections.
    pub(crate) ftp: ftp::ServerState,
    /// SMTP and POP3 server connections.
    pub(crate) mail: email::ServerState,
    /// TFTP server: files being read, by (client, port).
    tftp_tx: BTreeMap<(Ipv4Addr, u16), Vec<u8>>,
    /// Hosts: the last router advertisement heard (router's address, prefixes).
    ra: Option<(std::net::Ipv6Addr, Vec<packetcraft_wire::ipv6::PrefixInfo>, u8)>,
    /// Hosts: DHCPv6 messages for a client exchange in flight.
    dhcp6_inbox: Vec<packetcraft_wire::dhcp6::Dhcp6>,
    /// Routers: DHCPv6 addresses handed out (address → client MAC, pool).
    pub dhcp6_leases: BTreeMap<std::net::Ipv6Addr, (MacAddr, String)>,
    /// Routers: IPsec security associations, by peer address.
    pub ipsec: BTreeMap<Ipv4Addr, vpn::IpsecSa>,
    /// Routers and multilayer switches: QoS class counters and policer buckets.
    pub(crate) qos: qos::QosCounters,
    /// The firewall appliance: connections and translations.
    pub fw: firewall::FwState,
}

/// One frame delivered across a cable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEvent {
    pub id: u64,
    pub at_us: u64,
    pub from: End,
    pub to: End,
    pub frame: Vec<u8>,
    pub protocol: Protocol,
    pub summary: String,
    /// Why the receiver discarded it, if it did.
    pub dropped: Option<String>,
    /// The IEEE 802.11 frame, when this hop crossed the air. `frame` then holds the Ethernet II
    /// frame it stands for (data, EAPOL), or nothing for management frames, which have none:
    /// a pcap file has one link type, so the Ethernet capture keeps every data hop and
    /// [`Sim::pcap_wlan`] gives the air hops as 802.11.
    pub air: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Queued {
    at: u64,
    seq: u64,
    from: End,
    to: End,
    frame: Vec<u8>,
    /// Switches crossed so far (simulator bookkeeping, not on the wire).
    hops: u8,
}

impl Ord for Queued {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.at, self.seq).cmp(&(o.at, o.seq))
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// Options of a ping.
#[derive(Debug, Clone, Copy)]
pub struct PingOptions {
    pub count: u16,
    pub timeout_us: u64,
    /// ICMP data bytes.
    pub size: usize,
    /// `None`: the device's default (128 on hosts, 255 on routers).
    pub ttl: Option<u8>,
}

impl Default for PingOptions {
    fn default() -> Self {
        PingOptions { count: 5, timeout_us: 2_000_000, size: 72, ttl: None }
    }
}

/// The outcome of one echo request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Reply { from: IpAddr, rtt_us: u64 },
    TtlExceeded { from: IpAddr, rtt_us: u64 },
    Unreachable { from: IpAddr, code: u8 },
    Timeout,
}

impl Probe {
    /// The one-character mark a ping line shows: `!` reply, `.` timeout, `U` unreachable,
    /// `T` TTL exceeded.
    pub fn mark(self) -> char {
        match self {
            Probe::Reply { .. } => '!',
            Probe::TtlExceeded { .. } => 'T',
            Probe::Unreachable { .. } => 'U',
            Probe::Timeout => '.',
        }
    }
}

/// One traceroute hop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop {
    pub ttl: u8,
    pub probes: Vec<Probe>,
}

/// The simulator.
#[derive(Debug, Clone, Default)]
pub struct Sim {
    now: u64,
    seq: u64,
    queue: BinaryHeap<Reverse<Queued>>,
    state: BTreeMap<DeviceId, DevState>,
    trace: VecDeque<TraceEvent>,
    next_event: u64,
    next_ip_id: u16,
    next_echo_id: u16,
    storm: bool,
    /// Spanning trees per VLAN, for the topology with this fingerprint.
    stp_for: u64,
    stp: BTreeMap<u16, stp::VlanTree>,
    /// Trees from before the last topology change, to see which ports started forwarding.
    stp_prev: BTreeMap<u16, stp::VlanTree>,
    /// Ports that started forwarding at this time (µs) and spend 30 s listening and learning.
    stp_since: BTreeMap<(u16, DeviceId, usize), u64>,
    /// EtherChannel bundles per switch, for the same topology as `stp`.
    chmaps: BTreeMap<DeviceId, channel::PortMap>,
    /// OSPF's routes, for the topology with this fingerprint.
    ospf_for: u64,
    ospf: Option<ospf::OspfState>,
    ospf6_for: u64,
    ospf6: Option<ospf6::Ospf6State>,
    rip_for: u64,
    rip: Option<rip::RipState>,
    eigrp_for: u64,
    eigrp: Option<eigrp::EigrpState>,
    /// EIGRP neighbours no longer heard, kept until their hold time runs out.
    eigrp_held: Vec<eigrp::Held>,
    /// The last EIGRP sequence number each router used.
    eigrp_seqs: BTreeMap<DeviceId, u32>,
    /// The flags of the last router advertisement a host autoconfigured from.
    last_ra_flags: u8,
    /// Control frames (BPDUs, hellos, updates) are queued up to this time.
    ctl_until: u64,
    /// OSPF adjacencies already announced on the wire: (router, interface, neighbour's router
    /// ID). A new one gets its database exchange.
    ospf_adjacent: std::collections::BTreeSet<(DeviceId, String, Ipv4Addr)>,
    /// Server files FTP changed, for the lab to apply (see [`Sim::take_ftp_changes`]).
    ftp_changes: Vec<(DeviceId, String, Option<String>)>,
    /// FTP data connections: (server, data port) → the client's port.
    ftp_data_peer: BTreeMap<(DeviceId, u16), u16>,
    /// Mailbox changes mail servers made, for the lab to apply.
    mail_changes: Vec<(DeviceId, email::MailChange)>,
    /// SNMP sets agents accepted: (device, object name, value), for the lab to apply.
    snmp_changes: Vec<(DeviceId, &'static str, String)>,
    /// SNMPv3 localized keys: (hash, password, engine ID) → key.
    snmp_keys: BTreeMap<(u8, String, Vec<u8>), Vec<u8>>,
    /// Messages SMTP servers took for other domains: (server, message), to relay.
    mail_relay: Vec<(DeviceId, packetcraft_model::services::Mail)>,
    /// Locally raised packets wait for ARP instead of being dropped (see `transmit`).
    hold_for_arp: bool,
    hsrp_for: u64,
    hsrp: Option<BTreeMap<DeviceId, Vec<hsrp::Role>>>,
    /// Last known link state of every router and switch port, for link messages.
    links_seen: BTreeMap<(DeviceId, usize), bool>,
    /// Lab changes waiting for the engine.
    events: Vec<PortEvent>,
    /// Wi-Fi associations as the air has shown them, by host radio.
    assoc: BTreeMap<End, wlan::Assoc>,
    /// Lightweight APs' joins as CAPWAP has shown them.
    joined: BTreeMap<DeviceId, wlan::Joined>,
    /// 802.1X logins waiting for a RADIUS answer, by the controller's source port.
    dot1x: BTreeMap<u16, wlan::Dot1xWait>,
    /// WPA2-Personal master keys by (passphrase, SSID).
    pmks: BTreeMap<(String, String), [u8; 32]>,
    air_seq: u16,
    capwap_seq: u8,
}

impl Sim {
    pub fn new() -> Sim {
        Sim::default()
    }

    /// Simulated time, in microseconds.
    pub fn now_us(&self) -> u64 {
        self.now
    }

    /// A device's clock: Unix seconds now, and whether someone set it (`clock set`). An unset
    /// clock starts at 2026-01-01 00:00:00 UTC when the simulation starts.
    pub fn clock(&self, d: DeviceId) -> (i64, bool) {
        let now = i64::try_from(self.now / 1_000_000).unwrap_or(i64::MAX);
        match self.state.get(&d).and_then(|s| s.clock_base) {
            Some(base) => (base.saturating_add(now), true),
            None => (DEFAULT_CLOCK.saturating_add(now), false),
        }
    }

    /// Adds a message to a device's logging buffer and its console, stamped with its clock.
    pub fn log(&mut self, d: DeviceId, msg: &str) {
        let (unix, _) = self.clock(d);
        let secs = unix.rem_euclid(86_400);
        let line = format!("[{:02}:{:02}:{:02}] {msg}", secs / 3_600, secs / 60 % 60, secs % 60);
        let now = self.now;
        let st = self.st(d);
        if st.log.len() >= MAX_LOG {
            st.log.pop_front();
        }
        st.log.push_back((now, line.clone()));
        if st.syslog_out.len() < MAX_LOG {
            st.syslog_out.push(msg.to_string());
        }
        if st.console.len() < MAX_LOG {
            st.console.push(line);
        }
    }

    /// Messages for a device's console since the last call.
    pub fn take_console(&mut self, d: DeviceId) -> Vec<String> {
        self.state.get_mut(&d).map(|s| std::mem::take(&mut s.console)).unwrap_or_default()
    }

    /// Devices with console messages waiting.
    pub fn console_pending(&self) -> Vec<DeviceId> {
        self.state.iter().filter(|(_, s)| !s.console.is_empty()).map(|(id, _)| *id).collect()
    }

    pub fn clear_log(&mut self, d: DeviceId) {
        if let Some(s) = self.state.get_mut(&d) {
            s.log.clear();
        }
    }

    /// Seconds left of a `login block-for` quiet period on a device (`None`: logins allowed).
    pub fn login_blocked(&self, d: DeviceId) -> Option<u64> {
        let until = self.state.get(&d)?.login_blocked_until;
        (until > self.now).then(|| (until - self.now).div_ceil(1_000_000))
    }

    /// Records a failed remote login; with `login block-for`, enough failures in the window start
    /// the quiet period. Returns its length in seconds when it starts.
    pub fn login_failed(&mut self, d: DeviceId, block: Option<(u32, u32, u32)>) -> Option<u32> {
        let now = self.now;
        let st = self.st(d);
        let (secs, attempts, within) = block?;
        let window = u64::from(within).saturating_mul(1_000_000);
        st.login_failures.retain(|t| now.saturating_sub(*t) < window);
        st.login_failures.push(now);
        if st.login_failures.len() >= attempts.max(1) as usize {
            st.login_failures.clear();
            st.login_blocked_until = now.saturating_add(u64::from(secs).saturating_mul(1_000_000));
            return Some(secs);
        }
        None
    }

    pub fn set_debug(&mut self, d: DeviceId, what: DebugKind, on: bool) {
        let st = self.st(d);
        match what {
            DebugKind::Icmp => st.debug_icmp = on,
            DebugKind::Rip => st.debug_rip = on,
            DebugKind::Ospf => st.debug_ospf = on,
            DebugKind::Eigrp => st.debug_eigrp = on,
            DebugKind::Packet => st.debug_packet = on,
            DebugKind::Nat => st.debug_nat = on,
            DebugKind::Dhcp => st.debug_dhcp = on,
        }
    }

    /// The debugs that are on, by name.
    pub fn debugs(&self, d: DeviceId) -> Vec<&'static str> {
        let Some(s) = self.state.get(&d) else { return Vec::new() };
        [
            (s.debug_icmp, "ICMP"),
            (s.debug_rip, "RIP"),
            (s.debug_ospf, "OSPF"),
            (s.debug_eigrp, "EIGRP"),
            (s.debug_packet, "IP packets"),
            (s.debug_nat, "NAT"),
            (s.debug_dhcp, "DHCP server"),
        ]
        .into_iter()
        .filter(|x| x.0)
        .map(|x| x.1)
        .collect()
    }

    /// Logs link changes on routers and switches since the last look.
    fn note_links(&mut self, t: &Topology) {
        let mut now: Vec<((DeviceId, usize), bool)> = Vec::new();
        for d in t.devices.iter().filter(|d| matches!(d.kind, DeviceKind::Router | DeviceKind::Switch)) {
            for (i, p) in d.ports.iter().enumerate() {
                let end = End { device: d.id, port: i };
                if t.peer(end).is_some() || !p.shutdown {
                    now.push(((d.id, i), port_up(t, end)));
                }
            }
        }
        let first = self.links_seen.is_empty();
        let mut seen = BTreeMap::new();
        for (key, up) in now {
            if !first
                && let Some(before) = self.links_seen.get(&key)
                && *before != up
                && let Some(p) = t.device(key.0).and_then(|d| d.ports.get(key.1))
            {
                let msg = format!("{}: link {}", p.name, if up { "up" } else { "down" });
                self.log(key.0, &msg);
                self.queue_trap(key.0, snmp::TrapEvent::Link(key.1, up));
            }
            seen.insert(key, up);
        }
        self.links_seen = seen;
    }

    /// `clock set`: the device's clock reads `unix` now.
    pub fn set_clock(&mut self, d: DeviceId, unix: i64) {
        let now = i64::try_from(self.now / 1_000_000).unwrap_or(i64::MAX);
        self.st(d).clock_base = Some(unix.saturating_sub(now));
    }

    pub fn state(&self, d: DeviceId) -> Option<&DevState> {
        self.state.get(&d)
    }

    pub fn trace(&self) -> &VecDeque<TraceEvent> {
        &self.trace
    }

    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    /// Whether the last run had to stop a broadcast storm.
    pub fn storm_detected(&self) -> bool {
        self.storm
    }

    /// Forgets all dynamic state and events (power-cycles the whole lab).
    pub fn reset(&mut self) {
        *self = Sim { next_event: self.next_event, ..Sim::default() };
    }

    /// Forgets one device's dynamic state (it reloaded).
    pub fn reset_device(&mut self, d: DeviceId) {
        self.state.remove(&d);
        // It starts again: a coldStart trap, when traps are on.
        self.queue_trap(d, snmp::TrapEvent::ColdStart);
    }

    /// Empties a switch's MAC table.
    pub fn clear_macs(&mut self, d: DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.macs.clear();
        }
    }

    /// Empties a device's ARP cache.
    pub fn clear_arp(&mut self, d: DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.arp.clear();
            st.arp_asked.clear();
        }
    }

    /// Drops state the new topology invalidates: removed devices, entries on ports that went
    /// down, and frames in flight on removed cables.
    pub fn topology_changed(&mut self, t: &Topology) {
        self.state.retain(|id, _| t.device(*id).is_some());
        self.note_links(t);
        // Ports reset by `shutdown`/`no shutdown`, or no longer secured, forget what they learned.
        for (id, st) in &mut self.state {
            if let Some(d) = t.device(*id) {
                st.secure.retain(|p, _| d.ports.get(*p).is_some_and(|x| x.port_security.is_some() && !x.err_disabled && !x.shutdown));
            }
        }
        for (id, st) in &mut self.state {
            let Some(d) = t.device(*id) else { continue };
            st.macs.retain(|_, e| port_up(t, End { device: *id, port: e.port }));
            let ifs = up_ifaces(t, d);
            st.arp.retain(|ip, e| ifs.iter().any(|i| i.name == e.iface && i.net.contains(*ip)));
            st.pending.retain(|p| ifs.iter().any(|i| i.name == p.iface.name));
            let ifs6 = v6::up_ifaces6(t, d);
            st.nd.retain(|_, e| ifs6.iter().any(|i| i.name == e.iface));
            st.pending6.retain(|p| ifs6.iter().any(|i| i.name == p.iface()));
        }
        let queue = std::mem::take(&mut self.queue);
        let joining = |e: End, ap: End| t.wifi_attempt(e).is_some_and(|a| a.ap == ap);
        self.queue = queue
            .into_iter()
            .filter(|Reverse(q)| t.peer(q.from) == Some(q.to) || (wlan::air_hop(t, q.from, q.to) && (joining(q.from, q.to) || joining(q.to, q.from))))
            .collect();
        self.wlan_sync(t);
        if self.eigrp.is_some() || t.devices.iter().any(|d| d.eigrp.is_some()) {
            self.eigrp_refresh(t);
        }
    }

    /// A capture of the event list, in pcap format.
    pub fn pcap(&self) -> Vec<u8> {
        packetcraft_wire::pcap::write(self.trace.iter().filter(|e| !e.frame.is_empty()).map(|e| (e.at_us, e.frame.as_slice())))
    }

    fn st(&mut self, d: DeviceId) -> &mut DevState {
        self.state.entry(d).or_default()
    }

    fn emit(&mut self, t: &Topology, from: End, frame: Vec<u8>) -> bool {
        self.emit_hops(t, from, frame, 0)
    }

    fn emit_hops(&mut self, t: &Topology, from: End, frame: Vec<u8>, hops: u8) -> bool {
        if !port_up(t, from) || self.err_disabled_now(from.device, from.port) {
            return false;
        }
        // Over the air an access point reaches every associated host; each keeps what is its own.
        if is_ap_radio(t, from) {
            // Not back to the host that sent it; a frame for one host goes to that host only.
            let src = frame.get(6..12).map(<[u8]>::to_vec).unwrap_or_default();
            let dst = frame.get(0..6).map(<[u8]>::to_vec).unwrap_or_default();
            let clients = t.wifi_clients(from);
            let mac = |e: &End| t.device(e.device).and_then(|d| d.ports.get(e.port)).map(|p| p.mac.0.to_vec()).unwrap_or_default();
            let unicast: Vec<End> = clients.iter().filter(|c| mac(c) == dst).copied().collect();
            for to in if unicast.is_empty() { clients } else { unicast } {
                if mac(&to) == src {
                    continue;
                }
                self.push_hop(t, self.now + LINK_DELAY_US, from, to, frame.clone(), hops);
            }
            return true;
        }
        let Some(to) = t.peer(from) else { return false };
        if self.err_disabled_now(to.device, to.port) {
            return false;
        }
        self.push_hop(t, self.now + LINK_DELAY_US, from, to, frame, hops);
        true
    }

    /// Processes events up to `deadline`, stopping early when `stop` says so. Returns whether
    /// it stopped early.
    fn advance(&mut self, t: &Topology, deadline: u64, mut stop: impl FnMut(&Sim) -> bool) -> bool {
        self.storm = false;
        self.refresh_stp(t);
        self.wlan_sync(t);
        self.flush_syslog(t);
        self.flush_traps(t);
        self.schedule_control(t, deadline);
        let mut count = 0usize;
        loop {
            let next = self.queue.peek().map(|Reverse(top)| top.at).filter(|at| *at <= deadline);
            // An EIGRP neighbour's hold time running out is an event of its own.
            if let Some(h) = self.eigrp_next_expiry().filter(|h| *h <= deadline && next.is_none_or(|q| *h <= q)) {
                self.now = self.now.max(h);
                self.eigrp_refresh(t);
                // Expired holds are gone after a refresh; never spin on one that isn't.
                self.eigrp_held.retain(|x| x.until > h);
                continue;
            }
            if next.is_none() {
                break;
            }
            let Some(Reverse(q)) = self.queue.pop() else { break };
            self.now = self.now.max(q.at);
            self.deliver(t, q);
            count += 1;
            if count >= MAX_EVENTS_PER_RUN {
                self.storm = true;
                self.queue.clear();
                break;
            }
            if stop(self) {
                return true;
            }
        }
        self.now = self.now.max(deadline);
        false
    }

    /// Runs the simulation for `us` microseconds.
    pub fn run_for(&mut self, t: &Topology, us: u64) {
        let deadline = self.now.saturating_add(us);
        self.advance(t, deadline, |_| false);
    }

    /// Runs until nothing is in flight (at most [`MAX_EVENTS_PER_RUN`] events).
    pub fn settle(&mut self, t: &Topology) {
        let mut budget = MAX_EVENTS_PER_RUN;
        while let Some(Reverse(top)) = self.queue.peek() {
            let at = top.at;
            let before = self.next_event;
            self.advance(t, at.saturating_add(10_000_000), |_| false);
            let used = usize::try_from(self.next_event.saturating_sub(before)).unwrap_or(usize::MAX);
            budget = budget.saturating_sub(used.max(1));
            if self.storm || budget == 0 {
                self.storm = self.storm || !self.queue.is_empty();
                self.queue.clear();
                break;
            }
        }
    }

    fn deliver(&mut self, t: &Topology, q: Queued) {
        // Serial frames are listed (and captured) as the equivalent Ethernet II frame, since a
        // capture file has one link type; the summary says which framing the cable carried.
        let serial = t.is_serial(q.to).then(|| packetcraft_wire::serial::decode(&q.frame).ok()).flatten();
        let mac = |e: End| t.device(e.device).and_then(|d| d.ports.get(e.port)).map_or(MacAddr::ZERO, |p| p.mac);
        let shown = match &serial {
            Some((_, et, payload)) => Frame { dst: mac(q.to), src: mac(q.from), vlan: None, ethertype: *et, payload: payload.clone() }.encode(),
            None => q.frame.clone(),
        };
        let air = wlan::air_hop(t, q.from, q.to);
        // Over the air: listed by what the 802.11 frame carries (its Ethernet equivalent).
        let shown = if air {
            packetcraft_wire::wlan::Dot11::decode(&q.frame).ok().and_then(|d| d.ethernet()).map(|e| e.encode()).unwrap_or_default()
        } else {
            shown
        };
        let (protocol, mut summary) =
            if air && shown.is_empty() { (Protocol::Wlan, packetcraft_wire::wlan::summarize(&q.frame)) } else { packetcraft_wire::summarize(&shown) };
        if let Some((f, ..)) = &serial {
            summary.push_str(if *f == packetcraft_wire::serial::Framing::Ppp { " [serial, PPP]" } else { " [serial, HDLC]" });
        }
        if air && !shown.is_empty() {
            summary.push_str(" [802.11]");
        }
        self.next_event += 1;
        if self.trace.len() >= TRACE_CAP {
            self.trace.pop_front();
        }
        self.trace.push_back(TraceEvent {
            id: self.next_event,
            at_us: q.at,
            from: q.from,
            to: q.to,
            frame: shown,
            protocol,
            summary,
            dropped: None,
            air: air.then(|| q.frame.clone()),
        });
        let dropped = if air { self.air_in(t, q.from, q.to, &q.frame, q.hops).err() } else { self.receive(t, q.to, &q.frame, q.hops).err() };
        if let (Some(reason), Some(ev)) = (dropped, self.trace.back_mut()) {
            ev.dropped = Some(reason);
        }
    }

    fn receive(&mut self, t: &Topology, at: End, bytes: &[u8], hops: u8) -> Result<(), String> {
        let d = t.device(at.device).ok_or("device removed")?;
        let port = d.ports.get(at.port).ok_or("port removed")?;
        if port.shutdown {
            return Err("port is shut down".into());
        }
        if ifname::is_serial(&port.name) {
            let (got, ethertype, payload) = packetcraft_wire::serial::decode(bytes).map_err(|e| e.to_string())?;
            if got != framing(port) {
                return Err(format!(
                    "{} framing on a {} port",
                    if got == packetcraft_wire::serial::Framing::Ppp { "PPP" } else { "HDLC" },
                    port.serial.encapsulation.name()
                ));
            }
            let f = Frame { dst: port.mac, src: MacAddr::ZERO, vlan: None, ethertype, payload };
            return self.l3_in(t, d, at.port, f);
        }
        let f = Frame::decode(bytes).map_err(|e| e.to_string())?;
        match d.kind {
            DeviceKind::Switch if port.routed => self.l3_in(t, d, at.port, f),
            DeviceKind::Switch => self.switch_in(t, d, at.port, f, hops),
            DeviceKind::Hub => self.hub_in(t, d, at.port, bytes, hops),
            _ => self.l3_in(t, d, at.port, f),
        }
    }

    // ---- NAT ----

    /// Inside to outside: the translated packet (`None`: no rule applies, sent as is).
    fn nat_translate(&mut self, d: &Device, ip: &Ipv4) -> Result<Option<Ipv4>, String> {
        let id = flow_id(ip, true);
        let st = self.st(d.id);
        if let Some(e) = st.nat.iter().find(|e| {
            e.inside_local == ip.src && e.protocol == ip.protocol && (e.local_id.is_none() || e.local_id == id) && (e.fixed || e.outside == ip.dst)
        }) {
            return Ok(Some(rewrite(ip, true, e.inside_global, e.global_id)));
        }
        let entry = if let Some((_, global)) = d.nat.statics.iter().find(|(l, _)| *l == ip.src) {
            NatEntry {
                protocol: ip.protocol,
                inside_local: ip.src,
                local_id: None,
                inside_global: *global,
                global_id: None,
                outside: ip.dst,
                fixed: true,
            }
        } else {
            let key = packetcraft_model::acl::PacketKey { protocol: ip.protocol, src: ip.src, dst: ip.dst, src_port: None, dst_port: None };
            let Some(rule) = d.nat.rules.iter().find(|r| d.acls.iter().find(|a| a.name == r.acl).is_some_and(|a| a.evaluate(&key).0)) else {
                return Ok(None);
            };
            let (global, pat) = match &rule.target {
                packetcraft_model::NatTarget::Interface(name) => {
                    let ifc = l3_ifaces(d)
                        .into_iter()
                        .find(|i| packetcraft_model::ifname::parse(name).is_some_and(|n| n.to_string() == i.name))
                        .ok_or_else(|| format!("NAT interface {name} has no address"))?;
                    (ifc.net.addr, true)
                }
                packetcraft_model::NatTarget::Pool(name) => {
                    let pool = d.nat.pools.iter().find(|p| &p.name == name).ok_or_else(|| format!("NAT pool {name} doesn't exist"))?;
                    let st = self.st(d.id);
                    let (a, b) = (u32::from(pool.start), u32::from(pool.end));
                    if rule.overload {
                        (pool.start, true)
                    } else {
                        let mine = st.nat.iter().find(|e| e.inside_local == ip.src && !e.fixed).map(|e| e.inside_global);
                        let free = mine.or_else(|| (a..=b).take(65_536).map(Ipv4Addr::from).find(|g| !st.nat.iter().any(|e| e.inside_global == *g)));
                        (free.ok_or_else(|| format!("NAT pool {name} is exhausted"))?, false)
                    }
                }
            };
            let global_id = if pat {
                let st = self.st(d.id);
                id.map(|want| {
                    let taken = |c: u16| st.nat.iter().any(|e| e.inside_global == global && e.global_id == Some(c) && e.protocol == ip.protocol);
                    if !taken(want) { want } else { (1024..=u16::MAX).find(|c| !taken(*c)).unwrap_or(want) }
                })
            } else {
                None
            };
            NatEntry {
                protocol: ip.protocol,
                inside_local: ip.src,
                local_id: if pat { id } else { None },
                inside_global: global,
                global_id,
                outside: ip.dst,
                fixed: false,
            }
        };
        let out = rewrite(ip, true, entry.inside_global, entry.global_id);
        if self.state.get(&d.id).is_some_and(|s| s.debug_nat) {
            self.log(d.id, &format!("NAT debug: new translation, source {} → {}, to {}", ip.src, entry.inside_global, ip.dst));
        }
        let st = self.st(d.id);
        if st.nat.len() >= MAX_NAT_ENTRIES {
            st.nat.remove(0);
        }
        st.nat.push(entry);
        Ok(Some(out))
    }

    /// Outside to inside: the packet with its inside address restored, if it matches a
    /// translation (or a static one).
    fn nat_untranslate(&mut self, d: &Device, ip: &Ipv4) -> Option<Ipv4> {
        if let Some(back) = self.nat_untranslate_error(d, ip) {
            return Some(back);
        }
        let id = flow_id(ip, false);
        let st = self.state.get(&d.id);
        if let Some(e) = st.and_then(|s| {
            s.nat.iter().find(|e| e.inside_global == ip.dst && e.protocol == ip.protocol && (e.global_id.is_none() || e.global_id == id))
        }) {
            return Some(rewrite(ip, false, e.inside_local, e.local_id));
        }
        d.nat.statics.iter().find(|(_, g)| *g == ip.dst).map(|(l, _)| rewrite(ip, false, *l, None))
    }

    /// An ICMP error about a packet NAT translated on its way out: the error goes back to the
    /// inside host, and the packet it quotes is translated back too (RFC 3022 §4.3), so
    /// traceroute and "unreachable" work through NAT.
    fn nat_untranslate_error(&self, d: &Device, ip: &Ipv4) -> Option<Ipv4> {
        if ip.protocol != PROTO_ICMP {
            return None;
        }
        let (code, mut original, time_exceeded) = match Icmp::decode(&ip.payload).ok()? {
            Icmp::TimeExceeded { original } => (0, original, true),
            Icmp::Unreachable { code, original } => (code, original, false),
            _ => return None,
        };
        let ihl = usize::from(original.first()? & 0x0f) * 4;
        let proto = *original.get(9)?;
        let src = Ipv4Addr::new(*original.get(12)?, *original.get(13)?, *original.get(14)?, *original.get(15)?);
        // The quoted flow id: the echo id, or the source port.
        let id_at = if proto == PROTO_ICMP { ihl + 4 } else { ihl };
        let quoted_id = u16::from_be_bytes([*original.get(id_at)?, *original.get(id_at + 1)?]);
        let e = self
            .state
            .get(&d.id)?
            .nat
            .iter()
            .find(|e| e.inside_global == src && e.protocol == proto && e.global_id.is_none_or(|g| g == quoted_id))
            .cloned()
            .or_else(|| {
                d.nat.statics.iter().find(|(_, g)| *g == src).map(|(l, g)| NatEntry {
                    protocol: proto,
                    inside_local: *l,
                    local_id: None,
                    inside_global: *g,
                    global_id: None,
                    outside: ip.src,
                    fixed: true,
                })
            })?;
        original.get_mut(12..16)?.copy_from_slice(&e.inside_local.octets());
        if let Some(local) = e.local_id {
            original.get_mut(id_at..id_at + 2)?.copy_from_slice(&local.to_be_bytes());
        }
        let msg = if time_exceeded { Icmp::TimeExceeded { original } } else { Icmp::Unreachable { code, original } };
        Some(Ipv4 { dst: e.inside_local, payload: msg.encode(), ..ip.clone() })
    }

    /// Whether NAT makes this router answer for an address (static or pool globals).
    fn nat_owns(d: &Device, a: Ipv4Addr) -> bool {
        d.nat.statics.iter().any(|(_, g)| *g == a) || d.nat.pools.iter().any(|p| (u32::from(p.start)..=u32::from(p.end)).contains(&u32::from(a)))
    }

    /// Forgets dynamic translations (`clear ip nat translation *`).
    pub fn clear_nat(&mut self, d: DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.nat.clear();
        }
    }

    // ---- access lists ----

    /// Applies the interface's inbound or outbound access list. A denied packet is dropped and
    /// its sender told "administratively prohibited".
    fn acl_check(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4, raw: &[u8], inbound: bool) -> Result<(), String> {
        let Some(port) = d.ports.get(ifc.port) else { return Ok(()) };
        let groups = match ifc.sub {
            Some(n) => port.subif(n).map(|s| &s.access),
            None => Some(&port.access),
        };
        let Some(name) = groups.and_then(|g| if inbound { g.inbound.as_ref() } else { g.outbound.as_ref() }) else { return Ok(()) };
        // An applied list that doesn't exist permits everything, as on real routers.
        let Some(acl) = d.acls.iter().find(|a| &a.name == name) else { return Ok(()) };
        let (src_port, dst_port) = match ip.protocol {
            PROTO_UDP => Udp::decode(&ip.payload, ip.src, ip.dst).map_or((None, None), |u| (Some(u.src_port), Some(u.dst_port))),
            PROTO_TCP => Tcp::decode(&ip.payload, ip.src, ip.dst).map_or((None, None), |u| (Some(u.src_port), Some(u.dst_port))),
            _ => (None, None),
        };
        let key = packetcraft_model::acl::PacketKey { protocol: ip.protocol, src: ip.src, dst: ip.dst, src_port, dst_port };
        let (permit, entry) = acl.evaluate(&key);
        let st = self.st(d.id);
        *st.acl_hits.entry((acl.name.clone(), entry.unwrap_or(usize::MAX))).or_insert(0) += 1;
        if permit {
            return Ok(());
        }
        self.icmp_error(t, d, ifc, ip, Icmp::Unreachable { code: packetcraft_wire::UNREACH_ADMIN, original: Icmp::quote(raw) });
        Err(format!("denied by access list {} ({})", acl.name, if inbound { "in" } else { "out" }))
    }

    /// Forgets access-list match counts (`clear access-list counters`).
    pub fn clear_acl_hits(&mut self, d: DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.acl_hits.clear();
        }
    }

    // ---- routing ----

    /// OSPF's routes for a device (cached per topology).
    fn ospf_routes(&mut self, t: &Topology, d: &Device) -> Vec<ospf::OspfRoute> {
        if d.ospf.is_none() {
            return Vec::new();
        }
        let f = ospf::fingerprint(t);
        if self.ospf.is_none() || self.ospf_for != f {
            self.ospf_for = f;
            self.ospf = Some(ospf::compute(t));
        }
        self.ospf.as_ref().and_then(|s| s.routes.get(&d.id).cloned()).unwrap_or_default()
    }

    pub fn route(&mut self, t: &Topology, d: &Device, dst: Ipv4Addr) -> Option<(L3If, Ipv4Addr)> {
        self.route_flow(t, d, Ipv4Addr::UNSPECIFIED, dst)
    }

    /// [`Sim::route`] for a flow: among equal paths, the one this source and destination take.
    pub fn route_flow(&mut self, t: &Topology, d: &Device, src: Ipv4Addr, dst: Ipv4Addr) -> Option<(L3If, Ipv4Addr)> {
        let e = self.eigrp_routes(t, d);
        let o = self.ospf_routes(t, d);
        let r = self.rip_routes(t, d);
        route_with(t, d, dst, flow_hash(src, dst), &e, &o, &r)
    }

    /// The routing table as the simulation has it now (EIGRP neighbours within their hold time
    /// included): what `show ip route` prints.
    pub fn routing_table(&mut self, t: &Topology, d: &Device) -> Vec<RouteEntry> {
        let e = self.eigrp_routes(t, d);
        let o = self.ospf_routes(t, d);
        let r = self.rip_routes(t, d);
        routing_table_with(t, d, &e, &o, &r)
    }

    /// EIGRP's routes for a device (cached per topology and hold timers).
    fn eigrp_routes(&mut self, t: &Topology, d: &Device) -> Vec<eigrp::EigrpRoute> {
        if d.eigrp.is_none() {
            return Vec::new();
        }
        self.eigrp_refresh(t);
        self.eigrp.as_ref().and_then(|s| s.routes.get(&d.id).cloned()).unwrap_or_default()
    }

    /// EIGRP's view of the lab (neighbours, topology tables, routes).
    pub fn eigrp_state(&mut self, t: &Topology) -> eigrp::EigrpState {
        self.eigrp_refresh(t);
        self.eigrp.clone().unwrap_or_default()
    }

    /// RIP's routes for a device (cached per topology).
    fn rip_routes(&mut self, t: &Topology, d: &Device) -> Vec<rip::RipRoute> {
        if d.rip.is_none() {
            return Vec::new();
        }
        self.rip_state(t).routes.get(&d.id).cloned().unwrap_or_default()
    }

    /// Every device's HSRP roles (cached per topology).
    pub fn hsrp_roles(&mut self, t: &Topology) -> BTreeMap<DeviceId, Vec<hsrp::Role>> {
        let f = hsrp::fingerprint(t);
        if self.hsrp.is_none() || self.hsrp_for != f {
            self.hsrp_for = f;
            self.hsrp = Some(hsrp::compute(t));
        }
        self.hsrp.clone().unwrap_or_default()
    }

    /// Groups this device is active for on an interface: (virtual address, virtual MAC).
    fn hsrp_active(&mut self, t: &Topology, d: &Device, iface: Option<&str>) -> Vec<(Ipv4Addr, MacAddr)> {
        if !d.ports.iter().any(|p| !p.standby.is_empty() || p.subifs.iter().any(|s| !s.standby.is_empty()))
            && !d.svis.iter().any(|s| !s.standby.is_empty())
        {
            return Vec::new();
        }
        self.hsrp_roles(t)
            .get(&d.id)
            .into_iter()
            .flatten()
            .filter(|r| r.state == hsrp::State::Active && iface.is_none_or(|i| r.iface == i))
            .map(|r| (r.virtual_ip, r.virtual_mac()))
            .collect()
    }

    /// RIP's view of the lab (neighbours, routes).
    pub fn rip_state(&mut self, t: &Topology) -> rip::RipState {
        let f = ospf::fingerprint(t);
        if self.rip.is_none() || self.rip_for != f {
            self.rip_for = f;
            self.rip = Some(rip::compute(t));
        }
        self.rip.clone().unwrap_or_default()
    }

    /// OSPF's view of the lab (neighbours, router ids, routes).
    /// OSPFv3's view of the lab, recomputed when what it depends on changes.
    pub fn ospf6_state(&mut self, t: &Topology) -> ospf6::Ospf6State {
        let f = ospf6::fingerprint(t);
        if self.ospf6.is_none() || self.ospf6_for != f {
            self.ospf6_for = f;
            self.ospf6 = Some(ospf6::compute(t));
        }
        self.ospf6.clone().unwrap_or_default()
    }

    /// A device's dynamic IPv6 routes (OSPFv3), as routing-table lines.
    pub fn dynamic_routes6(&mut self, t: &Topology, d: &Device) -> Vec<v6::Route6Entry> {
        if d.ospf6.is_none() {
            return Vec::new();
        }
        let st = self.ospf6_state(t);
        st.routes
            .get(&d.id)
            .into_iter()
            .flatten()
            .map(|r| v6::Route6Entry {
                code: if r.external {
                    'E'
                } else if r.inter_area {
                    'I'
                } else {
                    'O'
                },
                net: r.net,
                iface: Some(r.iface.clone()),
                next_hop: Some(r.via),
            })
            .collect()
    }

    pub fn ospf_state(&mut self, t: &Topology) -> ospf::OspfState {
        let f = ospf::fingerprint(t);
        if self.ospf.is_none() || self.ospf_for != f {
            self.ospf_for = f;
            self.ospf = Some(ospf::compute(t));
        }
        self.ospf.clone().unwrap_or_default()
    }

    // ---- port security ----

    /// Admits a source address on a secured port, or applies the violation mode.
    fn port_security(&mut self, d: &Device, port: usize, src: MacAddr) -> Result<(), String> {
        let Some(ps) = d.ports.get(port).and_then(|p| p.port_security.as_ref()) else { return Ok(()) };
        if ps.macs.contains(&src) {
            return Ok(());
        }
        let st = self.st(d.id);
        let learned = st.secure.entry(port).or_default();
        if learned.contains(&src) {
            return Ok(());
        }
        if ps.macs.len() + learned.len() < usize::from(ps.max.max(1)) {
            learned.push(src);
            if ps.sticky {
                self.events.push(PortEvent::Sticky { device: d.id, port, mac: src });
            }
            return Ok(());
        }
        match ps.violation {
            packetcraft_model::Violation::Protect => Err(format!("port security: {src} is one address too many (dropped)")),
            packetcraft_model::Violation::Restrict => {
                *self.st(d.id).violations.entry(port).or_insert(0) += 1;
                Err(format!("port security: {src} is one address too many (dropped and counted)"))
            }
            packetcraft_model::Violation::Shutdown => {
                *self.st(d.id).violations.entry(port).or_insert(0) += 1;
                if !self.events.contains(&PortEvent::ErrDisabled { device: d.id, port }) {
                    self.events.push(PortEvent::ErrDisabled { device: d.id, port });
                    let name = d.ports.get(port).map_or(String::new(), |p| p.name.clone());
                    self.log(d.id, &format!("{name}: port security violation by {src}; port shut down (err-disabled)"));
                }
                Err(format!("port security violation from {src}: the port is shut down (err-disabled)"))
            }
        }
    }

    /// Lab changes the simulation decided (err-disabled ports, sticky addresses).
    pub fn take_events(&mut self) -> Vec<PortEvent> {
        std::mem::take(&mut self.events)
    }

    /// Whether a violation already shut this port in this run (before the lab records it).
    fn err_disabled_now(&self, d: DeviceId, port: usize) -> bool {
        self.events.contains(&PortEvent::ErrDisabled { device: d, port })
    }

    // ---- spanning tree ----

    fn refresh_stp(&mut self, t: &Topology) {
        let f = stp::fingerprint(t);
        if f != self.stp_for {
            self.stp_for = f;
            for (v, tree) in std::mem::take(&mut self.stp) {
                self.stp_prev.insert(v, tree);
            }
            self.chmaps.clear();
            // A topology change: switches flush what they learned, as a topology-change
            // notification makes them do, so no address points at a path that is gone.
            for st in self.state.values_mut() {
                st.macs.clear();
            }
        }
    }

    /// The spanning tree of a VLAN for this topology.
    pub fn spanning_tree(&mut self, t: &Topology, vlan: VlanId) -> &stp::VlanTree {
        self.refresh_stp(t);
        self.stp_tree(t, vlan)
    }

    /// The tree of a VLAN (computed once per topology). Ports that just became root or
    /// designated, without portfast or rapid spanning tree, start listening and learning.
    fn stp_tree(&mut self, t: &Topology, vlan: VlanId) -> &stp::VlanTree {
        let v = vlan.get();
        if !self.stp.contains_key(&v) {
            let tree = stp::compute(t, vlan);
            if let Some(prev) = self.stp_prev.remove(&v) {
                for ((dev, p), info) in &tree.ports {
                    let key = (v, *dev, *p);
                    if !info.role.forwards() {
                        self.stp_since.remove(&key);
                        continue;
                    }
                    // An EtherChannel is one port whichever member leads it now.
                    let members: Vec<usize> = t
                        .device(*dev)
                        .and_then(|d| {
                            let group = d.ports.get(*p)?.channel?.number;
                            Some((0..d.ports.len()).filter(|i| d.ports.get(*i).and_then(|x| x.channel).is_some_and(|g| g.number == group)).collect())
                        })
                        .unwrap_or_else(|| vec![*p]);
                    let was = members.iter().any(|m| prev.ports.get(&(*dev, *m)).is_some_and(|i| i.role.forwards()));
                    // Portfast, rapid spanning tree, and access-point radios forward at once.
                    let immediate =
                        t.device(*dev).is_some_and(|d| d.rapid_stp || d.ports.get(*p).is_some_and(|x| x.portfast || ifname::is_wireless(&x.name)));
                    if !was && !immediate {
                        self.stp_since.insert(key, self.now);
                    }
                }
            }
            self.stp.insert(v, tree);
        }
        self.stp.entry(v).or_insert_with(|| stp::compute(t, vlan))
    }

    /// A port that is still listening or learning: `"LIS"` the first 15 s, `"LRN"` the next 15.
    pub fn stp_transition(&self, dev: DeviceId, port: usize, vlan: VlanId) -> Option<&'static str> {
        let since = *self.stp_since.get(&(vlan.get(), dev, port))?;
        match self.now.saturating_sub(since) {
            0..15_000_000 => Some("LIS"),
            15_000_000..30_000_000 => Some("LRN"),
            _ => None,
        }
    }

    /// Ends every listening and learning period at once (a lab built offline starts converged;
    /// the time bar's fast-forward does the same).
    pub fn converge_stp(&mut self) {
        self.stp_since.clear();
    }

    /// Whether a port is listening or learning in any VLAN (an amber light).
    pub fn stp_converging(&self, dev: DeviceId, port: usize) -> bool {
        self.stp_since.iter().any(|((_, d, p), since)| *d == dev && *p == port && self.now.saturating_sub(*since) < 30_000_000)
    }

    fn chmap(&mut self, t: &Topology, d: &Device) -> channel::PortMap {
        self.chmaps.entry(d.id).or_insert_with(|| channel::PortMap::new(t, d)).clone()
    }

    fn stp_forwards(&mut self, t: &Topology, dev: DeviceId, port: usize, vlan: VlanId) -> bool {
        self.stp_tree(t, vlan).forwards(dev, port) && self.stp_transition(dev, port, vlan).is_none()
    }

    // ---- switching (IEEE 802.1D learning, 802.1Q tagging) ----

    fn switch_in(&mut self, t: &Topology, d: &Device, in_port: usize, f: Frame, hops: u8) -> Result<(), String> {
        if hops >= MAX_SWITCH_HOPS {
            self.storm = true;
            return Err(format!("switching loop: the frame crossed {MAX_SWITCH_HOPS} switches (no spanning tree)"));
        }
        // Spanning tree takes its BPDUs (the tree itself is computed), LLDP its advertisements.
        if packetcraft_wire::control::link_local(f.dst) {
            return Ok(());
        }
        let sp = &d.ports.get(in_port).ok_or("no port")?.switchport;
        let vlan = match if t.is_trunk(End { device: d.id, port: in_port }) { SwitchportMode::Trunk } else { SwitchportMode::Access } {
            SwitchportMode::Access | SwitchportMode::DynamicAuto | SwitchportMode::DynamicDesirable => {
                if f.vlan.is_some_and(|v| v != sp.access_vlan.get()) {
                    return Err("tagged frame on an access port".into());
                }
                sp.access_vlan
            }
            SwitchportMode::Trunk => {
                let v = match f.vlan {
                    Some(v) => VlanId::new(v).ok_or("invalid VLAN tag")?,
                    None => sp.native_vlan,
                };
                if !sp.allowed.contains(v) {
                    return Err(format!("VLAN {v} not allowed on this trunk"));
                }
                v
            }
        };
        if !d.vlans.contains_key(&vlan) {
            return Err(format!("VLAN {vlan} does not exist on this switch"));
        }
        let chm = self.chmap(t, d);
        if chm.suspended(in_port) {
            return Err("port suspended: it can't join its EtherChannel".into());
        }
        if !self.stp_forwards(t, d.id, chm.logical(in_port), vlan) {
            if let Some(state) = self.stp_transition(d.id, chm.logical(in_port), vlan) {
                let what = if state == "LIS" { "listening" } else { "learning" };
                return Err(format!("port still {what} (spanning tree, VLAN {vlan}): it forwards 30 s after coming up"));
            }
            return Err(format!("port blocked by spanning tree (VLAN {vlan})"));
        }
        self.port_security(d, in_port, f.src)?;
        self.switch_forward(t, d, Some(in_port), vlan, f, hops)
    }

    /// Learns, delivers to the switch's own VLAN interface when addressed to it, and forwards
    /// within `vlan`. `in_port` is `None` for frames the switch itself sends.
    fn switch_forward(&mut self, t: &Topology, d: &Device, in_port: Option<usize>, vlan: VlanId, f: Frame, hops: u8) -> Result<(), String> {
        let now = self.now;
        // A bundle is one port: learn on it, never send back into it, flood it once.
        let chm = self.chmap(t, d);
        let in_port = in_port.map(|p| chm.logical(p));
        let st = self.st(d.id);
        if let Some(p) = in_port
            && !f.src.is_group()
            && (st.macs.len() < MAX_MAC_ENTRIES || st.macs.contains_key(&(vlan.get(), f.src)))
        {
            st.macs.insert((vlan.get(), f.src), MacEntry { port: p, learned_us: now });
        }
        let known = if f.dst.is_group() {
            None
        } else {
            st.macs.get(&(vlan.get(), f.dst)).filter(|e| now.saturating_sub(e.learned_us) < MAC_AGING_US).map(|e| e.port)
        };
        // The switch's own VLAN interface.
        if in_port.is_some()
            && f.ethertype != packetcraft_wire::ipv6::ETHERTYPE_IPV6
            && let Some(svi) = up_ifaces(t, d).into_iter().find(|i| i.vlan == Some(vlan.get()))
        {
            if f.dst == svi.mac {
                return match f.ethertype {
                    ETHERTYPE_ARP => self.arp_in(t, d, &svi, &f.payload),
                    ETHERTYPE_IPV4 => self.ip_in(t, d, &svi, &f.payload),
                    other => Err(format!("ethertype 0x{other:04x} not understood")),
                };
            }
            if f.dst.is_broadcast() && f.ethertype == ETHERTYPE_ARP {
                let _ = self.arp_in(t, d, &svi, &f.payload);
            }
            // IPv4 broadcasts (a DHCP request, for one) reach the switch's own interface too,
            // and keep flooding the VLAN.
            if f.dst.is_broadcast() && f.ethertype == ETHERTYPE_IPV4 {
                let _ = self.ip_in(t, d, &svi, &f.payload);
            }
        }
        // IPv6 for the VLAN interface: its own address, or a group it may have joined (and
        // the frame still floods on).
        if in_port.is_some()
            && f.ethertype == packetcraft_wire::ipv6::ETHERTYPE_IPV6
            && let Some(svi6) = v6::up_ifaces6(t, d).into_iter().find(|i| i.port == SVI_PORT && i.vlan == Some(vlan.get()))
        {
            if f.dst == svi6.mac {
                return self.ip6_on(t, d, &svi6, &f);
            }
            if f.dst.is_group() {
                let _ = self.ip6_on(t, d, &svi6, &f);
            }
        }
        let outs: Vec<usize> = match known {
            // An access point relays between hosts on its radio.
            Some(p) if Some(p) == in_port && !is_ap_radio(t, End { device: d.id, port: p }) => {
                return Err("destination is on the arrival port (filtered)".into());
            }
            Some(p) => vec![p],
            None => (0..d.ports.len())
                .filter(|&p| {
                    (Some(p) != in_port || is_ap_radio(t, End { device: d.id, port: p }))
                        && !chm.hidden(p)
                        && !chm.suspended(p)
                        && carries(t, d, p, vlan)
                })
                .collect(),
        };
        let outs: Vec<usize> = outs.into_iter().filter(|&p| self.stp_forwards(t, d.id, p, vlan)).collect();
        let mut sent = 0;
        for p in outs {
            let p = chm.pick(p, f.src, f.dst);
            let Some(port) = d.ports.get(p) else { continue };
            let tag = if !t.is_trunk(End { device: d.id, port: p }) || port.switchport.native_vlan == vlan { None } else { Some(vlan.get()) };
            if self.emit_hops(t, End { device: d.id, port: p }, f.retagged(tag).encode(), hops + 1) {
                sent += 1;
            }
        }
        if sent == 0 { Err(format!("no up port in VLAN {vlan} to forward to")) } else { Ok(()) }
    }

    /// Sends a frame out of a layer-3 interface: a port, or a switch's VLAN interface.
    fn l3_emit(&mut self, t: &Topology, d: &Device, ifc: &L3If, frame: Frame) {
        if vpn::is_tunnel(ifc.port) {
            if frame.ethertype == ETHERTYPE_IPV4 {
                let _ = self.gre_out(t, d, ifc, frame.payload);
            }
            return;
        }
        if ifc.port == SVI_PORT {
            if let Some(v) = ifc.vlan.and_then(VlanId::new) {
                let _ = self.switch_forward(t, d, None, v, frame.retagged(None), 0);
            }
        } else {
            self.emit_frame(t, d, ifc.port, &frame);
        }
    }

    /// Sends a frame out of a port: as is on Ethernet, re-framed (HDLC or PPP) on serial.
    pub(crate) fn emit_frame(&mut self, t: &Topology, d: &Device, port: usize, frame: &Frame) {
        let end = End { device: d.id, port };
        match d.ports.get(port).filter(|p| ifname::is_serial(&p.name)) {
            Some(p) => {
                if let Ok(bytes) = packetcraft_wire::serial::encode(framing(p), frame.ethertype, &frame.payload) {
                    self.emit(t, end, bytes);
                }
            }
            None => {
                self.emit(t, end, frame.encode());
            }
        }
    }

    // ---- hubs: physical-layer repeaters ----

    fn hub_in(&mut self, t: &Topology, d: &Device, in_port: usize, bytes: &[u8], hops: u8) -> Result<(), String> {
        if hops >= MAX_SWITCH_HOPS {
            self.storm = true;
            return Err(format!("loop: the frame crossed {MAX_SWITCH_HOPS} hubs or switches"));
        }
        let mut sent = 0;
        for p in (0..d.ports.len()).filter(|&p| p != in_port) {
            if self.emit_hops(t, End { device: d.id, port: p }, bytes.to_vec(), hops + 1) {
                sent += 1;
            }
        }
        if sent == 0 { Err("no other port is up".into()) } else { Ok(()) }
    }

    // ---- hosts and routers ----

    fn l3_in(&mut self, t: &Topology, d: &Device, in_port: usize, f: Frame) -> Result<(), String> {
        if f.ethertype == packetcraft_wire::ipv6::ETHERTYPE_IPV6 {
            return self.ip6_in(t, d, in_port, &f);
        }
        // Spanning-tree BPDUs and LLDP stay on the link (neighbours are computed).
        if packetcraft_wire::control::link_local(f.dst) {
            return Ok(());
        }
        // A host (or a router's `ip address dhcp` port) waiting for DHCP takes answers before
        // it has an address.
        if self.state.get(&d.id).is_some_and(|s| s.dhcp_xid.is_some() && (d.kind.is_host() || s.dhcp_port == Some(in_port)))
            && (f.dst.is_broadcast() || d.ports.get(in_port).is_some_and(|p| p.mac == f.dst))
            && let Some(msg) = dhcp_in_frame(&f)
        {
            return self.dhcp_client_in(d, msg);
        }
        let ifc = l3_ifaces(d).into_iter().find(|i| i.port == in_port && i.vlan == f.vlan).ok_or_else(|| match f.vlan {
            Some(v) => format!("no subinterface for VLAN {v}"),
            None => "the interface has no IP address".to_string(),
        })?;
        // Unicast to us, broadcast, or an IPv4 multicast group (the IP layer decides on those).
        let ipv4_group = f.dst.is_group() && f.dst.0[..3] == [0x01, 0x00, 0x5e] && f.ethertype == ETHERTYPE_IPV4;
        let virtual_mac = !f.dst.is_group() && f.dst != ifc.mac && self.hsrp_active(t, d, Some(&ifc.name)).iter().any(|(_, m)| *m == f.dst);
        if f.dst != ifc.mac && !f.dst.is_broadcast() && !ipv4_group && !virtual_mac {
            return Err("not addressed to this device".into());
        }
        match f.ethertype {
            ETHERTYPE_ARP => self.arp_in(t, d, &ifc, &f.payload),
            ETHERTYPE_IPV4 => self.ip_in(t, d, &ifc, &f.payload),
            other => Err(format!("ethertype 0x{other:04x} not understood")),
        }
    }

    fn arp_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, payload: &[u8]) -> Result<(), String> {
        let a = Arp::decode(payload).map_err(|e| e.to_string())?;
        // An off-subnet sender is only accepted as the answer to our own question (a proxy
        // ARP reply for a route out of this interface).
        let asked = a.op == ArpOp::Reply && self.state.get(&d.id).is_some_and(|st| st.arp_asked.contains_key(&a.sender_ip));
        if !ifc.net.contains(a.sender_ip) && !asked {
            return Err(format!("ARP from {} is not on {}", a.sender_ip, ifc.net.network()));
        }
        // An HSRP virtual address this router is active for answers with the virtual MAC.
        let virtual_mac = self.hsrp_active(t, d, Some(&ifc.name)).into_iter().find(|(ip, _)| *ip == a.target_ip).map(|(_, m)| m);
        let for_me = a.target_ip == ifc.net.addr
            || virtual_mac.is_some()
            || (nat_side(d, ifc) == Some(packetcraft_model::NatSide::Outside) && Sim::nat_owns(d, a.target_ip))
            || (packetcraft_model::firewall::is_firewall(d) && Sim::fw_owns(d, ifc, a.target_ip));
        let now = self.now;
        let st = self.st(d.id);
        if (for_me || st.arp.contains_key(&a.sender_ip)) && (st.arp.len() < MAX_ARP_ENTRIES || st.arp.contains_key(&a.sender_ip)) {
            st.arp.insert(a.sender_ip, ArpEntry { mac: a.sender_mac, learned_us: now, iface: ifc.name.clone() });
        }
        // Packets that were waiting for this address can go now.
        let ready: Vec<Pending> = {
            let (ready, keep) = std::mem::take(&mut st.pending).into_iter().partition(|p| p.next_hop == a.sender_ip && p.iface.name == ifc.name);
            st.pending = keep;
            ready
        };
        for p in ready {
            let frame = Frame { dst: a.sender_mac, src: p.iface.mac, vlan: p.iface.vlan, ethertype: ETHERTYPE_IPV4, payload: p.datagram };
            self.l3_emit(t, d, &p.iface, frame);
        }
        if !for_me {
            // Proxy ARP (RFC 1027, on by default on routers): answer for an address we route
            // out of another interface.
            let proxied = a.op == ArpOp::Request
                && d.kind == DeviceKind::Router
                && !ifc.net.contains(a.target_ip)
                && self.route(t, d, a.target_ip).is_some_and(|(out, _)| out.name != ifc.name);
            if !proxied {
                return Err(format!("ARP for {}, not this device", a.target_ip));
            }
        }
        if a.op == ArpOp::Request {
            let mac = virtual_mac.unwrap_or(ifc.mac);
            let reply = Arp { op: ArpOp::Reply, sender_mac: mac, sender_ip: a.target_ip, target_mac: a.sender_mac, target_ip: a.sender_ip };
            let frame = Frame { dst: a.sender_mac, src: mac, vlan: ifc.vlan, ethertype: ETHERTYPE_ARP, payload: reply.encode() };
            self.l3_emit(t, d, ifc, frame);
        }
        Ok(())
    }

    fn ip_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, payload: &[u8]) -> Result<(), String> {
        let mut ip = Ipv4::decode(payload).map_err(|e| e.to_string())?;
        // Routing-protocol messages to the link's routers (OSPF, RIP); hosts ignore them.
        let udp_port = if ip.protocol == PROTO_UDP { Udp::decode(&ip.payload, ip.src, ip.dst).ok().map(|u| u.dst_port) } else { None };
        let rip = udp_port == Some(packetcraft_wire::control::RIP_PORT);
        let hsrp = udp_port == Some(packetcraft_wire::control::HSRP_PORT);
        if (ip.dst.octets()[..3] == [224, 0, 0]) || (rip && ip.dst == Ipv4Addr::BROADCAST) {
            if d.kind.is_host() {
                return Ok(());
            }
            let debug = self.state.get(&d.id).map_or((false, false), |s| (s.debug_rip, s.debug_ospf));
            if debug.1
                && ip.protocol == packetcraft_wire::control::PROTO_OSPF
                && d.ospf.is_some()
                && let Ok(h) = packetcraft_wire::control::OspfHello::decode(&ip.payload)
            {
                let msg = format!(
                    "OSPF debug: hello from {} (router ID {}, area {}) on {}, {} neighbour{} listed",
                    ip.src,
                    h.router_id,
                    h.area,
                    ifc.name,
                    h.neighbors.len(),
                    if h.neighbors.len() == 1 { "" } else { "s" }
                );
                self.log(d.id, &msg);
            }
            if ip.protocol == packetcraft_wire::eigrp::PROTO_EIGRP
                && d.eigrp.is_some()
                && self.state.get(&d.id).is_some_and(|s| s.debug_eigrp)
                && let Ok(p) = packetcraft_wire::eigrp::EigrpPacket::decode(&ip.payload)
            {
                let msg = match packetcraft_wire::eigrp::EigrpHello::decode(&ip.payload) {
                    Ok(h) => format!("EIGRP debug: hello from {} on {}, AS {}, hold time {} s", ip.src, ifc.name, h.asn, h.hold_secs),
                    Err(_) if p.opcode == packetcraft_wire::eigrp::OPCODE_HELLO => {
                        format!("EIGRP debug: acknowledgement of {} from {} on {}, AS {}", p.ack, ip.src, ifc.name, p.asn)
                    }
                    Err(_) => format!(
                        "EIGRP debug: {} #{} from {} on {}, AS {}, {} route{}",
                        packetcraft_wire::eigrp::opcode_name(p.opcode),
                        p.seq,
                        ip.src,
                        ifc.name,
                        p.asn,
                        p.routes().count(),
                        if p.routes().count() == 1 { "" } else { "s" }
                    ),
                };
                self.log(d.id, &msg);
            }
            if debug.0 && rip && d.rip.is_some() {
                let entries = Udp::decode(&ip.payload, ip.src, ip.dst).ok().and_then(|u| packetcraft_wire::control::rip_decode(&u.payload).ok());
                if let Some((v, routes)) = entries {
                    let listed: Vec<String> = routes
                        .iter()
                        .take(8)
                        .map(|r| format!("{}/{} metric {}", r.net, packetcraft_core::mask_to_prefix(r.mask).unwrap_or(0), r.metric))
                        .collect();
                    let more = if routes.len() > 8 { format!(" and {} more", routes.len() - 8) } else { String::new() };
                    self.log(d.id, &format!("RIP debug: version {v} update from {} on {}: {}{more}", ip.src, ifc.name, listed.join(", ")));
                }
            }
            return match (ip.protocol, rip) {
                (packetcraft_wire::control::PROTO_OSPF, _) if d.ospf.is_some() => Ok(()),
                (packetcraft_wire::control::PROTO_OSPF, _) => Err("OSPF is not running here".into()),
                (packetcraft_wire::eigrp::PROTO_EIGRP, _) if d.eigrp.is_some() => Ok(()),
                (packetcraft_wire::eigrp::PROTO_EIGRP, _) => Err("EIGRP is not running here".into()),
                (_, true) if d.rip.is_some() => Ok(()),
                (_, true) => Err("RIP is not running here".into()),
                _ if hsrp => Ok(()),
                _ => Err(format!("not listening to {}", ip.dst)),
            };
        }
        if packetcraft_model::firewall::is_firewall(d) {
            return self.fw_ip_in(t, d, ifc, ip, payload);
        }
        let mut from_outside = false;
        let marked;
        let mut payload = payload;
        if d.routes_ip() {
            self.acl_check(t, d, ifc, &ip, payload, true)?;
            // An input service policy: may re-mark the packet, or police it away.
            if !d.qos.applied.is_empty() {
                marked = self.qos_apply(d, &ifc.name, true, payload.to_vec())?;
                payload = &marked;
            }
            // NAT, outside to inside: undo the translation before deciding where it goes.
            if nat_side(d, ifc) == Some(packetcraft_model::NatSide::Outside)
                && let Some(back) = self.nat_untranslate(d, &ip)
            {
                ip = back;
                from_outside = true;
            }
        }
        let mine = l3_ifaces(d).iter().any(|i| i.net.addr == ip.dst) || self.hsrp_active(t, d, None).iter().any(|(v, _)| *v == ip.dst);
        if mine || ip.dst == Ipv4Addr::BROADCAST || (ifc.net.prefix < 31 && ip.dst == ifc.net.broadcast()) {
            return self.local_in(t, d, ifc, ip);
        }
        if !d.routes_ip() {
            return Err(format!("{} is not this device's address", ip.dst));
        }
        if ip.ttl <= 1 {
            self.icmp_error(t, d, ifc, &ip, Icmp::TimeExceeded { original: Icmp::quote(payload) });
            return Err("TTL expired in transit".into());
        }
        let fwd = Ipv4 { ttl: ip.ttl - 1, ..ip.clone() };
        let debug = self.state.get(&d.id).is_some_and(|s| s.debug_packet);
        match self.route_flow(t, d, ip.src, ip.dst) {
            Some((out, nh)) => {
                if debug {
                    let via = if nh == ip.dst { String::new() } else { format!(" via {nh}") };
                    self.log(d.id, &format!("IP debug: {} → {}, in {}, routed out {}{via}", ip.src, ip.dst, ifc.name, out.name));
                }
                self.acl_check(t, d, &out, &ip, payload, false)?;
                let mut fwd = fwd;
                if !from_outside
                    && nat_side(d, ifc) == Some(packetcraft_model::NatSide::Inside)
                    && nat_side(d, &out) == Some(packetcraft_model::NatSide::Outside)
                    && let Some(tr) = self.nat_translate(d, &fwd)?
                {
                    fwd = tr;
                }
                self.transmit(t, d, &out, nh, qos::keep_ds(payload, fwd.encode()))
            }
            None => {
                if debug {
                    self.log(d.id, &format!("IP debug: {} → {}, in {}, no route: dropped", ip.src, ip.dst, ifc.name));
                }
                self.icmp_error(t, d, ifc, &ip, Icmp::Unreachable { code: packetcraft_wire::UNREACH_NET, original: Icmp::quote(payload) });
                Err(format!("no route to {}", ip.dst))
            }
        }
    }

    fn local_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: Ipv4) -> Result<(), String> {
        if d.kind == DeviceKind::Router && ip.protocol == packetcraft_wire::vpn::PROTO_GRE {
            return self.gre_in(t, d, &ip);
        }
        if d.kind == DeviceKind::Router && ip.protocol == packetcraft_wire::vpn::PROTO_ESP {
            return self.esp_in(t, d, ifc, &ip);
        }
        if ip.protocol == PROTO_TCP {
            return self.tcp_in(t, d, &ip);
        }
        if ip.protocol == PROTO_UDP {
            let u = Udp::decode(&ip.payload, ip.src, ip.dst).map_err(|e| e.to_string())?;
            if d.kind == DeviceKind::Router && u.dst_port == packetcraft_wire::ike::ISAKMP_PORT && u.src_port == packetcraft_wire::ike::ISAKMP_PORT {
                return self.ike_in(t, d, &ip, &u.payload);
            }
            if let Some(r) = self.wlan_udp_in(t, d, &ip, &u) {
                return r;
            }
            if let Some(r) = self.mgmt_udp_in(t, d, ifc, &ip, &u) {
                return r;
            }
            return match (u.dst_port, d.kind) {
                (dhcp::SERVER_PORT, k) if k == DeviceKind::Router || (k == DeviceKind::Switch && d.routes_ip()) => {
                    let msg = Dhcp::decode(&u.payload).map_err(|e| e.to_string())?;
                    match msg.kind {
                        DhcpKind::Offer | DhcpKind::Ack | DhcpKind::Nak => self.dhcp_relay_down(t, d, msg),
                        _ if msg.relay.is_unspecified() => {
                            let helpers = helpers_of(d, ifc);
                            let local_pool = d.dhcp_pools.iter().any(|p| p.network.is_some_and(|n| n.contains(ifc.net.addr)));
                            if !helpers.is_empty() && !local_pool {
                                self.dhcp_relay_up(t, d, ifc, msg, &helpers)
                            } else {
                                self.dhcp_server_in(t, d, ifc, msg)
                            }
                        }
                        _ => self.dhcp_server_in(t, d, ifc, msg),
                    }
                }
                (dhcp::SERVER_PORT, DeviceKind::Server) if d.services.dhcp.on => {
                    let msg = Dhcp::decode(&u.payload).map_err(|e| e.to_string())?;
                    match msg.kind {
                        DhcpKind::Discover | DhcpKind::Request | DhcpKind::Release => self.dhcp_server_in(t, d, ifc, msg),
                        _ => Err("not a client message".into()),
                    }
                }
                (dns::PORT, DeviceKind::Server) if d.services.dns.on => {
                    let q = dns::Dns::decode(&u.payload).map_err(|e| e.to_string())?;
                    if q.response {
                        return Err("a DNS answer sent to a server".into());
                    }
                    let found = d.services.dns.lookup(&q.name);
                    let ans = dns::Dns {
                        id: q.id,
                        response: true,
                        rcode: if found.is_some() { 0 } else { dns::NXDOMAIN },
                        name: q.name,
                        answers: found.into_iter().collect(),
                    };
                    let src = if ip.dst == Ipv4Addr::BROADCAST { ifc.net.addr } else { ip.dst };
                    let reply = Udp { src_port: dns::PORT, dst_port: u.src_port, payload: ans.encode() };
                    let out = Ipv4 {
                        id: self.ip_id(),
                        ttl: default_ttl(d.kind),
                        protocol: PROTO_UDP,
                        src,
                        dst: ip.src,
                        payload: reply.encode(src, ip.src),
                    };
                    self.send_ip(t, d, out)
                }
                (port, k) if k.is_host() && u.src_port == dns::PORT => {
                    let ans = dns::Dns::decode(&u.payload).map_err(|e| e.to_string())?;
                    let st = self.st(d.id);
                    if st.dns_wait != Some((port, ans.id)) {
                        return Err("DNS answer nobody asked for".into());
                    }
                    st.dns_answer = Some(ans);
                    Ok(())
                }
                (dhcp::CLIENT_PORT, k) if k.is_host() => {
                    let msg = Dhcp::decode(&u.payload).map_err(|e| e.to_string())?;
                    self.dhcp_client_in(d, msg)
                }
                (port, _) => Err(format!("no service on UDP port {port}")),
            };
        }
        if ip.protocol != PROTO_ICMP {
            return Err(format!("no service for IP protocol {}", ip.protocol));
        }
        let icmp = Icmp::decode(&ip.payload).map_err(|e| e.to_string())?;
        let now = self.now;
        let mut note = |id: u16, seq: u16, kind: IcmpKind| {
            let st = self.st(d.id);
            if st.icmp.len() < 4_096 {
                st.icmp.push(IcmpEvent { at: now, from: ip.src, id, seq, kind });
            }
        };
        match icmp {
            Icmp::EchoRequest { id, seq, data } => {
                if self.state.get(&d.id).is_some_and(|s| s.debug_icmp) {
                    self.log(d.id, &format!("ICMP debug: echo request from {} to {} (id {id}, seq {seq})", ip.src, ip.dst));
                }
                let src = if ip.dst == Ipv4Addr::BROADCAST || ip.dst == ifc.net.broadcast() { ifc.net.addr } else { ip.dst };
                let reply = Ipv4 {
                    id: self.ip_id(),
                    ttl: default_ttl(d.kind),
                    protocol: PROTO_ICMP,
                    src,
                    dst: ip.src,
                    payload: Icmp::EchoReply { id, seq, data }.encode(),
                };
                self.send_ip(t, d, reply)
            }
            Icmp::EchoReply { id, seq, .. } => {
                note(id, seq, IcmpKind::Reply);
                if self.state.get(&d.id).is_some_and(|s| s.debug_icmp) {
                    self.log(d.id, &format!("ICMP debug: echo reply from {} (id {id}, seq {seq})", ip.src));
                }
                Ok(())
            }
            Icmp::TimeExceeded { original } => {
                if let Some((id, seq)) = Icmp::quoted_echo(&original) {
                    note(id, seq, IcmpKind::TimeExceeded);
                }
                Ok(())
            }
            Icmp::Unreachable { code, original } => {
                if let Some((id, seq)) = Icmp::quoted_echo(&original) {
                    note(id, seq, IcmpKind::Unreachable(code));
                }
                Ok(())
            }
        }
    }

    /// Sends an ICMP error about `offending` back to its source. Never about another ICMP error.
    fn icmp_error(&mut self, t: &Topology, d: &Device, ifc: &L3If, offending: &Ipv4, msg: Icmp) {
        if offending.protocol == PROTO_ICMP && !matches!(Icmp::decode(&offending.payload), Ok(Icmp::EchoRequest { .. } | Icmp::EchoReply { .. })) {
            return;
        }
        let err =
            Ipv4 { id: self.ip_id(), ttl: default_ttl(d.kind), protocol: PROTO_ICMP, src: ifc.net.addr, dst: offending.src, payload: msg.encode() };
        let _ = self.send_ip(t, d, err);
    }

    fn ip_id(&mut self) -> u16 {
        self.next_ip_id = self.next_ip_id.wrapping_add(1);
        self.next_ip_id
    }

    /// Routes and sends a datagram this device originates.
    fn send_ip(&mut self, t: &Topology, d: &Device, ip: Ipv4) -> Result<(), String> {
        let (out, nh) = self.route_flow(t, d, ip.src, ip.dst).ok_or_else(|| format!("no route to {}", ip.dst))?;
        self.transmit(t, d, &out, nh, ip.encode())
    }

    /// Puts a datagram on the wire towards `nh`, resolving its MAC with ARP first.
    fn transmit(&mut self, t: &Topology, d: &Device, out: &L3If, nh: Ipv4Addr, datagram: Vec<u8>) -> Result<(), String> {
        // Into a GRE tunnel: encapsulated and routed again.
        if vpn::is_tunnel(out.port) {
            let datagram = if d.qos.applied.is_empty() { datagram } else { self.qos_apply(d, &out.name, false, datagram)? };
            return self.gre_out(t, d, out, datagram);
        }
        // Protected by a crypto map on this interface: sent to the IPsec peer as ESP.
        if !d.crypto.applied.is_empty()
            && let Some((esp, peer)) = self.crypto_out(t, d, out, &datagram)?
        {
            let (o2, nh2) = self.route(t, d, peer).ok_or_else(|| format!("no route to IPsec peer {peer}"))?;
            return self.transmit(t, d, &o2, nh2, esp);
        }
        // An output service policy, on the packet as it goes on the wire.
        let datagram = if d.qos.applied.is_empty() { datagram } else { self.qos_apply(d, &out.name, false, datagram)? };
        // A serial link is point to point: no address to resolve.
        if out.port != SVI_PORT && t.is_serial(End { device: d.id, port: out.port }) {
            let frame = Frame { dst: MacAddr::BROADCAST, src: out.mac, vlan: None, ethertype: ETHERTYPE_IPV4, payload: datagram };
            self.l3_emit(t, d, out, frame);
            return Ok(());
        }
        if nh == Ipv4Addr::BROADCAST || (out.net.prefix < 31 && nh == out.net.broadcast()) {
            let frame = Frame { dst: MacAddr::BROADCAST, src: out.mac, vlan: out.vlan, ethertype: ETHERTYPE_IPV4, payload: datagram };
            self.l3_emit(t, d, out, frame);
            return Ok(());
        }
        let now = self.now;
        let known = self.st(d.id).arp.get(&nh).filter(|e| now.saturating_sub(e.learned_us) < ARP_AGING_US && e.iface == out.name).map(|e| e.mac);
        if let Some(mac) = known {
            let frame = Frame { dst: mac, src: out.mac, vlan: out.vlan, ethertype: ETHERTYPE_IPV4, payload: datagram };
            self.l3_emit(t, d, out, frame);
            return Ok(());
        }
        // A notification the device itself raises (a trap) waits too: it is sent only once.
        let host = !d.routes_ip() || self.hold_for_arp;
        let st = self.st(d.id);
        st.pending.retain(|p| now.saturating_sub(p.since) < ARP_WAIT_US);
        // Hosts hold the packet until ARP answers; routers drop it, as real routers do.
        if host && st.pending.len() < MAX_PENDING {
            st.pending.push(Pending { next_hop: nh, iface: out.clone(), datagram, since: now });
        }
        let ask = st.arp_asked.get(&nh).is_none_or(|at| now.saturating_sub(*at) >= ARP_RETRY_US);
        if ask {
            if st.arp_asked.len() > MAX_ARP_ENTRIES {
                st.arp_asked.clear();
            }
            st.arp_asked.insert(nh, now);
            let req = Arp { op: ArpOp::Request, sender_mac: out.mac, sender_ip: out.net.addr, target_mac: MacAddr::ZERO, target_ip: nh };
            let frame = Frame { dst: MacAddr::BROADCAST, src: out.mac, vlan: out.vlan, ethertype: ETHERTYPE_ARP, payload: req.encode() };
            self.l3_emit(t, d, out, frame);
        }
        if host { Ok(()) } else { Err(format!("waiting for ARP of {nh}; packet dropped")) }
    }

    // ---- DHCP ----

    /// The address a pool gives `mac`: its current lease, else the address it already uses
    /// (leases aren't saved with the lab), else the first free one. Like real servers, an
    /// address some device already uses is never offered (conflict detection).
    fn dhcp_pick(
        &self,
        t: &Topology,
        d: &Device,
        pool: &packetcraft_model::DhcpPool,
        mac: MacAddr,
        allowed: Option<&[Ipv4Addr]>,
    ) -> Option<Ipv4Addr> {
        let net = pool.network?.network();
        let leases = self.state.get(&d.id).map(|s| &s.leases);
        if let Some((ip, _)) = leases.and_then(|l| l.iter().find(|(ip, l)| l.mac == mac && net.contains(**ip))) {
            return Some(*ip);
        }
        let in_use: Vec<(Ipv4Addr, MacAddr)> = t
            .devices
            .iter()
            .flat_map(|x| {
                x.ports
                    .iter()
                    .flat_map(|p| p.ip.map(|n| (n.addr, p.mac)).into_iter().chain(p.subifs.iter().filter_map(|s| s.ip.map(|n| (n.addr, p.mac)))))
            })
            .collect();
        if let Some((ip, _)) =
            in_use.iter().find(|(ip, m)| *m == mac && net.contains(*ip) && leases.is_none_or(|l| l.get(ip).is_none_or(|x| x.mac == mac)))
        {
            return Some(*ip);
        }
        let mut mine: Vec<Ipv4Addr> = l3_ifaces(d).iter().map(|i| i.net.addr).collect();
        mine.extend(in_use.iter().map(|(ip, _)| *ip));
        if let Some(list) = allowed {
            return list.iter().copied().find(|ip| !mine.contains(ip) && leases.is_none_or(|l| !l.contains_key(ip)));
        }
        let first = u32::from(net.addr).saturating_add(1);
        let last = u32::from(net.broadcast()).saturating_sub(1);
        (first..=last).take(MAX_POOL_SCAN as usize).map(Ipv4Addr::from).find(|ip| {
            !mine.contains(ip)
                && !d.dhcp_excluded.iter().any(|(a, b)| (u32::from(*a)..=u32::from(*b)).contains(&u32::from(*ip)))
                && leases.is_none_or(|l| !l.contains_key(ip))
        })
    }

    fn dhcp_server_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, msg: Dhcp) -> Result<(), String> {
        let anchor = if msg.relay.is_unspecified() { ifc.net.addr } else { msg.relay };
        // A server's pools are ranges (start address, count); a router's are whole networks.
        let (pool, allowed): (packetcraft_model::DhcpPool, Option<Vec<Ipv4Addr>>) = if d.kind == DeviceKind::Server {
            let sp = d
                .services
                .dhcp
                .pools
                .iter()
                .find(|p| Ipv4Net::with_mask(p.start, p.mask).is_ok_and(|n| n.contains(anchor)))
                .ok_or_else(|| format!("no DHCP pool for {}", ifc.net.network()))?;
            let net = Ipv4Net::with_mask(sp.start, sp.mask).map_err(|e| e.to_string())?.network();
            let nz = |a: Ipv4Addr| (!a.is_unspecified()).then_some(a);
            (
                packetcraft_model::DhcpPool { name: sp.name.clone(), network: Some(net), default_router: nz(sp.gateway), dns: nz(sp.dns) },
                Some(sp.addresses().collect()),
            )
        } else if packetcraft_model::firewall::is_firewall(d) {
            let (pool, allowed) = Sim::fw_dhcp_pool(d, ifc)?;
            (pool, Some(allowed))
        } else {
            let p = d
                .dhcp_pools
                .iter()
                .find(|p| p.network.is_some_and(|n| n.contains(anchor)))
                .ok_or_else(|| format!("no DHCP pool for {}", ifc.net.network()))?
                .clone();
            (p, None)
        };
        let net = pool.network.ok_or("pool without network")?;
        let mut reply = Dhcp::new(DhcpKind::Offer, msg.xid, msg.client_mac);
        reply.server_id = Some(ifc.net.addr);
        reply.mask = Some(net.mask());
        reply.router = pool.default_router;
        reply.dns = pool.dns;
        reply.lease_secs = Some(86_400);
        match msg.kind {
            DhcpKind::Discover => {
                reply.your_ip = self
                    .dhcp_pick(t, d, &pool, msg.client_mac, allowed.as_deref())
                    .ok_or_else(|| format!("pool {} has no free address", pool.name))?;
            }
            DhcpKind::Request => {
                if msg.server_id.is_some_and(|s| !l3_ifaces(d).iter().any(|i| i.net.addr == s)) {
                    return Err("request for another DHCP server".into());
                }
                let wanted = msg.requested.unwrap_or(msg.client_ip);
                let ok = self.dhcp_pick(t, d, &pool, msg.client_mac, allowed.as_deref()).is_some_and(|p| p == wanted)
                    || self.state.get(&d.id).is_some_and(|s| s.leases.get(&wanted).is_some_and(|l| l.mac == msg.client_mac));
                if ok {
                    let now = self.now;
                    self.st(d.id).leases.insert(wanted, Lease { mac: msg.client_mac, pool: pool.name.clone(), since_us: now });
                    if self.state.get(&d.id).is_some_and(|s| s.debug_dhcp) {
                        self.log(d.id, &format!("DHCP debug: {wanted} given to {} from pool {}", msg.client_mac, pool.name));
                    }
                    reply.kind = DhcpKind::Ack;
                    reply.your_ip = wanted;
                } else {
                    reply = Dhcp::new(DhcpKind::Nak, msg.xid, msg.client_mac);
                    reply.server_id = Some(ifc.net.addr);
                }
            }
            DhcpKind::Release => {
                let mac = msg.client_mac;
                if self.state.get(&d.id).is_some_and(|s| s.debug_dhcp) {
                    self.log(d.id, &format!("DHCP debug: {mac} released its address"));
                }
                self.st(d.id).leases.retain(|_, l| l.mac != mac);
                return Ok(());
            }
            _ => return Err("not a client message".into()),
        }
        if !msg.relay.is_unspecified() {
            // Through a relay: answer the relay agent, which broadcasts on the client's segment.
            reply.relay = msg.relay;
            let udp = Udp { src_port: dhcp::SERVER_PORT, dst_port: dhcp::SERVER_PORT, payload: reply.encode() };
            let ip = Ipv4 {
                id: self.ip_id(),
                ttl: 255,
                protocol: PROTO_UDP,
                src: ifc.net.addr,
                dst: msg.relay,
                payload: udp.encode(ifc.net.addr, msg.relay),
            };
            return self.send_ip(t, d, ip);
        }
        let udp = Udp { src_port: dhcp::SERVER_PORT, dst_port: dhcp::CLIENT_PORT, payload: reply.encode() };
        let ip = Ipv4 {
            id: self.ip_id(),
            ttl: 255,
            protocol: PROTO_UDP,
            src: ifc.net.addr,
            dst: Ipv4Addr::BROADCAST,
            payload: udp.encode(ifc.net.addr, Ipv4Addr::BROADCAST),
        };
        self.transmit(t, d, ifc, Ipv4Addr::BROADCAST, ip.encode())
    }

    /// Relay agent, client side: forward a client's broadcast to the helper addresses.
    fn dhcp_relay_up(&mut self, t: &Topology, d: &Device, ifc: &L3If, mut msg: Dhcp, helpers: &[Ipv4Addr]) -> Result<(), String> {
        msg.relay = ifc.net.addr;
        for h in helpers {
            let udp = Udp { src_port: dhcp::SERVER_PORT, dst_port: dhcp::SERVER_PORT, payload: msg.encode() };
            let ip = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_UDP, src: ifc.net.addr, dst: *h, payload: udp.encode(ifc.net.addr, *h) };
            self.send_ip(t, d, ip)?;
        }
        Ok(())
    }

    /// Relay agent, server side: broadcast the server's answer on the client's segment.
    fn dhcp_relay_down(&mut self, t: &Topology, d: &Device, msg: Dhcp) -> Result<(), String> {
        let ifc = l3_ifaces(d).into_iter().find(|i| i.net.addr == msg.relay).ok_or("DHCP answer for a relay address this router doesn't have")?;
        let udp = Udp { src_port: dhcp::SERVER_PORT, dst_port: dhcp::CLIENT_PORT, payload: msg.encode() };
        let ip = Ipv4 {
            id: self.ip_id(),
            ttl: 255,
            protocol: PROTO_UDP,
            src: ifc.net.addr,
            dst: Ipv4Addr::BROADCAST,
            payload: udp.encode(ifc.net.addr, Ipv4Addr::BROADCAST),
        };
        self.transmit(t, d, &ifc, Ipv4Addr::BROADCAST, ip.encode())
    }

    fn dhcp_client_in(&mut self, d: &Device, msg: Dhcp) -> Result<(), String> {
        let st = self.st(d.id);
        if st.dhcp_xid != Some(msg.xid) {
            return Err("DHCP answer for another client".into());
        }
        if st.dhcp_inbox.len() < 16 {
            st.dhcp_inbox.push(msg);
        }
        Ok(())
    }

    fn dhcp_send(&mut self, t: &Topology, d: &Device, msg: &Dhcp) {
        let at = self.state.get(&d.id).and_then(|s| s.dhcp_port).unwrap_or(0);
        let Some(port) = d.ports.get(at) else { return };
        let src = Ipv4Addr::UNSPECIFIED;
        let udp = Udp { src_port: dhcp::CLIENT_PORT, dst_port: dhcp::SERVER_PORT, payload: msg.encode() };
        let ip =
            Ipv4 { id: self.ip_id(), ttl: 128, protocol: PROTO_UDP, src, dst: Ipv4Addr::BROADCAST, payload: udp.encode(src, Ipv4Addr::BROADCAST) };
        let frame = Frame { dst: MacAddr::BROADCAST, src: port.mac, vlan: None, ethertype: ETHERTYPE_IPV4, payload: ip.encode() };
        self.emit(t, End { device: d.id, port: at }, frame.encode());
    }

    fn dhcp_wait(&mut self, t: &Topology, dev: DeviceId, kinds: &[DhcpKind]) -> Option<Dhcp> {
        let deadline = self.now.saturating_add(2_000_000);
        let has = |s: &Sim| s.state.get(&dev).is_some_and(|st| st.dhcp_inbox.iter().any(|m| kinds.contains(&m.kind)));
        self.advance(t, deadline, has);
        let st = self.st(dev);
        let i = st.dhcp_inbox.iter().position(|m| kinds.contains(&m.kind))?;
        Some(st.dhcp_inbox.remove(i))
    }

    /// Runs a DHCP exchange for a host (DISCOVER, OFFER, REQUEST, ACK). The caller applies the
    /// result to the lab.
    pub fn dhcp_acquire(&mut self, t: &Topology, dev: DeviceId) -> Result<DhcpResult, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        if !d.kind.is_host() {
            return Err(SimError::NoSource("only hosts ask for addresses with DHCP".into()));
        }
        self.dhcp_acquire_port(t, dev, 0)
    }

    /// The same exchange from one port (`ip address dhcp` on a router or a routed port).
    pub fn dhcp_acquire_port(&mut self, t: &Topology, dev: DeviceId, port: usize) -> Result<DhcpResult, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        if !port_up(t, End { device: dev, port }) {
            return Err(SimError::NoSource("the network cable is not connected".into()));
        }
        let mac = d.ports.get(port).map(|p| p.mac).unwrap_or_default();
        self.seq += 1;
        let xid = (self.seq as u32) ^ (u32::from(mac.0[4]) << 8) ^ u32::from(mac.0[5]) << 16;
        {
            let st = self.st(dev);
            st.dhcp_xid = Some(xid);
            st.dhcp_port = Some(port);
            st.dhcp_inbox.clear();
        }
        let result = (|| {
            self.dhcp_send(t, d, &Dhcp::new(DhcpKind::Discover, xid, mac));
            let offer = self.dhcp_wait(t, dev, &[DhcpKind::Offer]).ok_or_else(|| SimError::NoSource("no DHCP server answered".into()))?;
            let mut req = Dhcp::new(DhcpKind::Request, xid, mac);
            req.requested = Some(offer.your_ip);
            req.server_id = offer.server_id;
            self.dhcp_send(t, d, &req);
            let ack = self
                .dhcp_wait(t, dev, &[DhcpKind::Ack, DhcpKind::Nak])
                .ok_or_else(|| SimError::NoSource("the DHCP server stopped answering".into()))?;
            if ack.kind == DhcpKind::Nak {
                return Err(SimError::NoSource("the DHCP server refused the request".into()));
            }
            let mask = ack.mask.or(offer.mask).unwrap_or(Ipv4Addr::new(255, 255, 255, 0));
            let address = Ipv4Net::with_mask(ack.your_ip, mask).map_err(|e| SimError::NoSource(e.to_string()))?;
            Ok(DhcpResult { address, router: ack.router, dns: ack.dns, server: ack.server_id })
        })();
        let st = self.st(dev);
        st.dhcp_xid = None;
        st.dhcp_port = None;
        st.dhcp_inbox.clear();
        result
    }

    /// Forgets a DHCP server's leases (`clear ip dhcp binding *`).
    pub fn clear_leases(&mut self, d: DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.leases.clear();
        }
    }

    // ---- TCP, HTTP and DNS ----

    /// TCP arriving at this device: the web server (port 80 on servers with HTTP on), or a
    /// segment for one of this host's client connections.
    fn tcp_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4) -> Result<(), String> {
        let seg = Tcp::decode(&ip.payload, ip.src, ip.dst).map_err(|e| e.to_string())?;
        if let Some(r) = self.ftp_server_in(t, d, ip, &seg) {
            return r;
        }
        if let Some(r) = self.mail_server_in(t, d, ip, &seg) {
            return r;
        }
        if let Some(r) = self.tacacs_server_in(t, d, ip, &seg) {
            return r;
        }
        if seg.dst_port == 80 && d.kind == DeviceKind::Server {
            return self.http_server_in(t, d, ip, &seg);
        }
        if matches!(seg.dst_port, 22 | 23) && matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) {
            return self.login_server_in(t, d, ip, &seg);
        }
        let st = self.st(d.id);
        if !st.tcp_wait.contains(&(seg.dst_port, seg.src_port)) {
            if seg.flags & tcp::RST == 0 {
                let rst = Tcp {
                    src_port: seg.dst_port,
                    dst_port: seg.src_port,
                    seq: seg.ack,
                    ack: seg.seq.wrapping_add(1),
                    flags: tcp::RST | tcp::ACK,
                    window: 0,
                    payload: Vec::new(),
                };
                let out = Ipv4 {
                    id: self.ip_id(),
                    ttl: default_ttl(d.kind),
                    protocol: PROTO_TCP,
                    src: ip.dst,
                    dst: ip.src,
                    payload: rst.encode(ip.dst, ip.src),
                };
                let _ = self.send_ip(t, d, out);
            }
            return Err(format!("no service on TCP port {}", seg.dst_port));
        }
        if st.tcp_inbox.len() < 64 {
            st.tcp_inbox.push(seg);
        }
        Ok(())
    }

    fn tcp_send(&mut self, t: &Topology, d: &Device, src: Ipv4Addr, dst: Ipv4Addr, seg: &Tcp) -> Result<(), String> {
        let out = Ipv4 { id: self.ip_id(), ttl: default_ttl(d.kind), protocol: PROTO_TCP, src, dst, payload: seg.encode(src, dst) };
        self.send_ip(t, d, out)
    }

    /// A stateless little web server: SYN → SYN-ACK, request → page + FIN, FIN → ACK.
    fn http_server_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Result<(), String> {
        let reply =
            |flags: u8, seq: u32, ack: u32, payload: Vec<u8>| Tcp { src_port: 80, dst_port: seg.src_port, seq, ack, flags, window: 65_535, payload };
        let isn = 50_000u32;
        if !d.services.http.on {
            let rst = reply(tcp::RST | tcp::ACK, 0, seg.seq.wrapping_add(1), Vec::new());
            self.tcp_send(t, d, ip.dst, ip.src, &rst)?;
            return Err("the web service is off".into());
        }
        if seg.flags & tcp::SYN != 0 {
            return self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::SYN | tcp::ACK, isn, seg.seq.wrapping_add(1), Vec::new()));
        }
        if !seg.payload.is_empty() {
            let request = String::from_utf8_lossy(&seg.payload).to_string();
            let path = request.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
            let name = path.trim_start_matches('/');
            let name = if name.is_empty() { "index.html" } else { name };
            let (status, body) = match d.services.http.files.iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
                Some(f) => ("200 OK", f.content.clone()),
                None => ("404 Not Found", format!("<html><body><h1>Not found</h1><p>This server has no file named {name}.</p></body></html>")),
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nServer: PacketCraft\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let ack = seg.seq.wrapping_add(seg.payload.len() as u32);
            let mut seq = isn.wrapping_add(1);
            let bytes = response.into_bytes();
            let chunks: Vec<&[u8]> = bytes.chunks(1460).collect();
            let last = chunks.len().saturating_sub(1);
            for (i, c) in chunks.into_iter().enumerate() {
                let flags = if i == last { tcp::PSH | tcp::ACK | tcp::FIN } else { tcp::ACK };
                self.tcp_send(t, d, ip.dst, ip.src, &reply(flags, seq, ack, c.to_vec()))?;
                seq = seq.wrapping_add(c.len() as u32);
            }
            return Ok(());
        }
        if seg.flags & tcp::FIN != 0 {
            return self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::ACK, seg.ack, seg.seq.wrapping_add(1), Vec::new()));
        }
        Ok(())
    }

    /// Remote logins: SYN-ACK when the vty lines accept the protocol, RST otherwise; data and
    /// FINs are acknowledged (the session itself runs in the console layer).
    fn login_server_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Result<(), String> {
        let proto = if seg.dst_port == 22 { packetcraft_model::RemoteProto::Ssh } else { packetcraft_model::RemoteProto::Telnet };
        let reply = |flags: u8, seq: u32, ack: u32| Tcp {
            src_port: seg.dst_port,
            dst_port: seg.src_port,
            seq,
            ack,
            flags,
            window: 4128,
            payload: Vec::new(),
        };
        if seg.flags & tcp::SYN != 0 {
            // `access-class`: the vty lines only take logins its list permits.
            if let Some(name) = &d.access.vty.access_class
                && let Some(acl) = d.acls.iter().find(|a| a.name == *name)
            {
                let key = packetcraft_model::acl::PacketKey {
                    protocol: PROTO_TCP,
                    src: ip.src,
                    dst: ip.dst,
                    src_port: Some(seg.src_port),
                    dst_port: Some(seg.dst_port),
                };
                let (permit, hit) = acl.evaluate(&key);
                *self.st(d.id).acl_hits.entry((name.clone(), hit.unwrap_or(usize::MAX))).or_insert(0) += 1;
                if !permit {
                    self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::RST | tcp::ACK, 0, seg.seq.wrapping_add(1)))?;
                    return Err(format!("login from {} refused by access-class {name}", ip.src));
                }
            }
            if d.access.accepts(proto) {
                return self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::SYN | tcp::ACK, 7_000, seg.seq.wrapping_add(1)));
            }
            self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::RST | tcp::ACK, 0, seg.seq.wrapping_add(1)))?;
            return Err(format!("{} logins are not accepted here", proto.name()));
        }
        if seg.flags & tcp::FIN != 0 || !seg.payload.is_empty() {
            let len = (seg.payload.len() as u32).max(u32::from(seg.flags & tcp::FIN != 0));
            return self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::ACK, seg.ack, seg.seq.wrapping_add(len)));
        }
        Ok(())
    }

    /// Opens a TCP connection (SYN, SYN-ACK, ACK) and closes it again; tells whether the far end
    /// accepted. Used by Telnet and SSH, whose sessions then run in the console layer.
    pub fn tcp_connect(&mut self, t: &Topology, dev: DeviceId, dst: Ipv4Addr, port: u16) -> Result<(), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        // A connection to one of our own addresses never touches a cable.
        if l3_ifaces(d).iter().any(|i| i.net.addr == dst) {
            let proto = if port == 22 { packetcraft_model::RemoteProto::Ssh } else { packetcraft_model::RemoteProto::Telnet };
            let open = matches!(port, 22 | 23) && matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) && d.access.accepts(proto);
            return if open { Ok(()) } else { Err(SimError::NoSource(format!("{dst} refused the connection"))) };
        }
        let src = self.source_for(t, d, dst)?;
        let local = self.next_port();
        let seg = |flags: u8, seq: u32, ack: u32| Tcp { src_port: local, dst_port: port, seq, ack, flags, window: 65_535, payload: Vec::new() };
        {
            let st = self.st(dev);
            st.tcp_wait.push((local, port));
        }
        let result = (|| {
            let mut answer = None;
            for _ in 0..4 {
                let _ = self.tcp_send(t, d, src, dst, &seg(tcp::SYN, 3_000, 0));
                answer = self.tcp_wait(t, dev, |s| s.dst_port == local && s.flags & (tcp::SYN | tcp::RST) != 0);
                if answer.is_some() {
                    break;
                }
            }
            let a = answer.ok_or_else(|| SimError::NoSource(format!("{dst} did not answer (connection timed out)")))?;
            if a.flags & tcp::RST != 0 {
                return Err(SimError::NoSource(format!("{dst} refused the connection")));
            }
            let _ = self.tcp_send(t, d, src, dst, &seg(tcp::ACK, 3_001, a.seq.wrapping_add(1)));
            Ok(())
        })();
        let st = self.st(dev);
        st.tcp_wait.retain(|w| *w != (local, port));
        st.tcp_inbox.retain(|s| s.dst_port != local);
        result
    }

    fn next_port(&mut self) -> u16 {
        self.seq += 1;
        49_152 + (self.seq % 16_000) as u16
    }

    /// Resolves a name with the host's DNS server (an address resolves to itself).
    pub fn dns_resolve(&mut self, t: &Topology, dev: DeviceId, name: &str) -> Result<Ipv4Addr, SimError> {
        if let Ok(a) = name.parse::<Ipv4Addr>() {
            return Ok(a);
        }
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let server = d.dns.ok_or_else(|| SimError::NoSource(format!("{name}: this host has no DNS server configured")))?;
        let src = self.source_for(t, d, server)?;
        for _ in 0..2 {
            let port = self.next_port();
            let id = self.ip_id();
            {
                let st = self.st(dev);
                st.dns_wait = Some((port, id));
                st.dns_answer = None;
            }
            let q = Udp { src_port: port, dst_port: dns::PORT, payload: dns::Dns::query(id, name).encode() };
            let ip = Ipv4 { id: self.ip_id(), ttl: default_ttl(d.kind), protocol: PROTO_UDP, src, dst: server, payload: q.encode(src, server) };
            let _ = self.send_ip(t, d, ip);
            let deadline = self.now.saturating_add(2_000_000);
            self.advance(t, deadline, |s| s.state.get(&dev).is_some_and(|st| st.dns_answer.is_some()));
            let st = self.st(dev);
            st.dns_wait = None;
            if let Some(a) = st.dns_answer.take() {
                return a.answers.first().copied().ok_or_else(|| SimError::NoSource(format!("{name}: the DNS server doesn't know this name")));
            }
        }
        Err(SimError::NoSource(format!("{name}: the DNS server {server} didn't answer")))
    }

    fn tcp_wait(&mut self, t: &Topology, dev: DeviceId, want: impl Fn(&Tcp) -> bool) -> Option<Tcp> {
        let deadline = self.now.saturating_add(2_000_000);
        let have = |s: &Sim| s.state.get(&dev).is_some_and(|st| st.tcp_inbox.iter().any(&want));
        self.advance(t, deadline, have);
        let st = self.st(dev);
        let i = st.tcp_inbox.iter().position(&want)?;
        Some(st.tcp_inbox.remove(i))
    }

    /// Fetches `path` from the web server at `server` over TCP port 80. Returns the status line
    /// and the body.
    pub fn http_get(&mut self, t: &Topology, dev: DeviceId, server: Ipv4Addr, host: &str, path: &str) -> Result<(String, String), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let src = self.source_for(t, d, server)?;
        let port = self.next_port();
        let isn = 1_000u32;
        let seg = |flags: u8, seq: u32, ack: u32, payload: Vec<u8>| Tcp { src_port: port, dst_port: 80, seq, ack, flags, window: 65_535, payload };
        {
            let st = self.st(dev);
            st.tcp_wait.push((port, 80));
        }
        let result = (|| {
            // The handshake (tried twice: the first SYN can be lost while routers resolve ARP).
            let mut synack = None;
            for _ in 0..4 {
                let _ = self.tcp_send(t, d, src, server, &seg(tcp::SYN, isn, 0, Vec::new()));
                synack = self.tcp_wait(t, dev, |s| s.dst_port == port && s.flags & (tcp::SYN | tcp::RST) != 0);
                if synack.is_some() {
                    break;
                }
            }
            let synack = synack.ok_or_else(|| SimError::NoSource(format!("{server} did not answer (request timed out)")))?;
            if synack.flags & tcp::RST != 0 {
                return Err(SimError::NoSource(format!("{server} refused the connection (no web service)")));
            }
            let srv_seq = synack.seq.wrapping_add(1);
            let _ = self.tcp_send(t, d, src, server, &seg(tcp::ACK, isn + 1, srv_seq, Vec::new()));
            let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: PacketCraft\r\nConnection: close\r\n\r\n").into_bytes();
            let req_len = request.len() as u32;
            let _ = self.tcp_send(t, d, src, server, &seg(tcp::PSH | tcp::ACK, isn + 1, srv_seq, request));
            let mut body = Vec::new();
            let mut last_seq;
            loop {
                let Some(s) = self.tcp_wait(t, dev, |s| s.dst_port == port && (!s.payload.is_empty() || s.flags & tcp::FIN != 0)) else {
                    return Err(SimError::NoSource("the server stopped answering".into()));
                };
                last_seq = s.seq.wrapping_add(s.payload.len() as u32);
                body.extend_from_slice(&s.payload);
                if s.flags & tcp::FIN != 0 || body.len() > 200_000 {
                    break;
                }
            }
            let _ = self.tcp_send(t, d, src, server, &seg(tcp::FIN | tcp::ACK, isn + 1 + req_len, last_seq.wrapping_add(1), Vec::new()));
            let text = String::from_utf8_lossy(&body).to_string();
            let (head, content) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
            let status = head.lines().next().unwrap_or("").split_once(' ').map_or("", |x| x.1).to_string();
            Ok((status, content.to_string()))
        })();
        let st = self.st(dev);
        st.tcp_wait.retain(|w| *w != (port, 80));
        st.tcp_inbox.retain(|s| s.dst_port != port);
        result
    }

    // ---- ping and traceroute ----

    /// The address a device sources its own traffic to `dst` from.
    pub fn source_for(&mut self, t: &Topology, d: &Device, dst: Ipv4Addr) -> Result<Ipv4Addr, SimError> {
        let ifs = l3_ifaces(d);
        let Some(first) = ifs.first() else {
            return Err(SimError::NoSource(format!("{} has no IP address configured", d.name)));
        };
        // A host always sources from its one address; with its cable out the echo just times out.
        if d.kind.is_host() && (first.net.contains(dst) || d.gateway.is_some_and(|g| first.net.contains(g))) {
            return Ok(first.net.addr);
        }
        self.route(t, d, dst).map(|(i, _)| i.net.addr).ok_or_else(|| SimError::NoSource(format!("{} has no route to {dst}", d.name)))
    }

    fn probe(&mut self, t: &Topology, dev: DeviceId, dst: Ipv4Addr, ttl: u8, seq: u16, size: usize, timeout: u64) -> Result<Probe, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let src = self.source_for(t, d, dst)?;
        self.next_echo_id = self.next_echo_id.wrapping_add(1);
        let id = self.next_echo_id;
        let ip = Ipv4 {
            id: self.ip_id(),
            ttl,
            protocol: PROTO_ICMP,
            src,
            dst,
            payload: Icmp::EchoRequest { id, seq, data: vec![0xab; size.min(1_472)] }.encode(),
        };
        // An echo to ourselves never touches a cable.
        if l3_ifaces(d).iter().any(|i| i.net.addr == dst) {
            return Ok(Probe::Reply { from: dst.into(), rtt_us: 0 });
        }
        let sent = self.now;
        if self.send_ip(t, d, ip).is_err() && !d.routes_ip() {
            return Ok(Probe::Timeout);
        }
        let matches = |s: &Sim| s.state.get(&dev).is_some_and(|st| st.icmp.iter().any(|e| e.id == id && e.seq == seq));
        self.advance(t, sent.saturating_add(timeout), matches);
        let st = self.st(dev);
        let found = st.icmp.iter().position(|e| e.id == id && e.seq == seq);
        let ev = found.map(|i| st.icmp.remove(i));
        st.icmp.retain(|e| sent.saturating_sub(e.at) < 60_000_000);
        Ok(match ev {
            Some(IcmpEvent { kind: IcmpKind::Reply, from, at, .. }) => Probe::Reply { from: from.into(), rtt_us: at.saturating_sub(sent) },
            Some(IcmpEvent { kind: IcmpKind::TimeExceeded, from, at, .. }) => {
                Probe::TtlExceeded { from: from.into(), rtt_us: at.saturating_sub(sent) }
            }
            Some(IcmpEvent { kind: IcmpKind::Unreachable(code), from, .. }) => Probe::Unreachable { from: from.into(), code },
            None => Probe::Timeout,
        })
    }

    /// Sends `count` echo requests one after another, each waiting for its answer or timeout.
    pub fn ping(&mut self, t: &Topology, dev: DeviceId, dst: Ipv4Addr, opts: PingOptions) -> Result<Vec<Probe>, SimError> {
        let count = opts.count.clamp(1, 1_000);
        let kind = t.device(dev).ok_or(SimError::NoDevice(dev))?.kind;
        let ttl = opts.ttl.unwrap_or(default_ttl(kind)).max(1);
        let mut out = Vec::with_capacity(usize::from(count));
        for seq in 1..=count {
            out.push(self.probe(t, dev, dst, ttl, seq, opts.size, opts.timeout_us)?);
        }
        Ok(out)
    }

    /// Probes with increasing TTL until the destination answers (or `max_hops`).
    pub fn traceroute(&mut self, t: &Topology, dev: DeviceId, dst: Ipv4Addr, max_hops: u8, timeout_us: u64) -> Result<Vec<Hop>, SimError> {
        let mut hops = Vec::new();
        for ttl in 1..=max_hops.clamp(1, 64) {
            let mut probes = Vec::new();
            for seq in 1..=3 {
                probes.push(self.probe(t, dev, dst, ttl, seq, 32, timeout_us)?);
            }
            let done = probes.iter().any(|p| matches!(p, Probe::Reply { .. } | Probe::Unreachable { .. }));
            hops.push(Hop { ttl, probes });
            if done {
                break;
            }
        }
        Ok(hops)
    }
}

fn default_ttl(kind: DeviceKind) -> u8 {
    if kind.is_host() { 128 } else { 255 }
}

/// Whether switch port `p` carries `vlan` (a routed port carries none).
fn carries(t: &Topology, d: &Device, p: usize, vlan: VlanId) -> bool {
    d.ports.get(p).filter(|port| !port.routed).is_some_and(|port| {
        if t.is_trunk(End { device: d.id, port: p }) { port.switchport.allowed.contains(vlan) } else { port.switchport.access_vlan == vlan }
    })
}

/// The DHCP message inside a frame, if it is one for a client.
fn dhcp_in_frame(f: &Frame) -> Option<Dhcp> {
    if f.ethertype != ETHERTYPE_IPV4 {
        return None;
    }
    let ip = Ipv4::decode(&f.payload).ok()?;
    if ip.protocol != PROTO_UDP {
        return None;
    }
    let u = Udp::decode(&ip.payload, ip.src, ip.dst).ok()?;
    if u.dst_port != dhcp::CLIENT_PORT {
        return None;
    }
    Dhcp::decode(&u.payload).ok()
}

/// The `ip helper-address` list of an interface.
fn helpers_of(d: &Device, ifc: &L3If) -> Vec<Ipv4Addr> {
    let Some(p) = d.ports.get(ifc.port) else { return Vec::new() };
    match ifc.sub {
        Some(n) => p.subif(n).map(|s| s.helpers.clone()).unwrap_or_default(),
        None => p.helpers.clone(),
    }
}

/// The NAT side of an interface.
fn nat_side(d: &Device, ifc: &L3If) -> Option<packetcraft_model::NatSide> {
    if ifc.port == SVI_PORT {
        return d.svis.iter().find(|s| Some(s.vlan.get()) == ifc.vlan)?.nat;
    }
    let p = d.ports.get(ifc.port)?;
    match ifc.sub {
        Some(n) => p.subif(n)?.nat,
        None => p.nat,
    }
}

/// The per-flow id NAT translates: the ICMP echo identifier, or the UDP source port (outbound)
/// / destination port (inbound).
fn flow_id(ip: &Ipv4, outbound: bool) -> Option<u16> {
    match ip.protocol {
        PROTO_ICMP => match Icmp::decode(&ip.payload).ok()? {
            Icmp::EchoRequest { id, .. } | Icmp::EchoReply { id, .. } => Some(id),
            _ => None,
        },
        PROTO_UDP => Udp::decode(&ip.payload, ip.src, ip.dst).ok().map(|u| if outbound { u.src_port } else { u.dst_port }),
        PROTO_TCP => Tcp::decode(&ip.payload, ip.src, ip.dst).ok().map(|u| if outbound { u.src_port } else { u.dst_port }),
        _ => None,
    }
}

/// The packet with its source (outbound) or destination (inbound) address, and the flow id if
/// given, replaced. Checksums are recomputed on encode.
fn rewrite(ip: &Ipv4, outbound: bool, addr: Ipv4Addr, id: Option<u16>) -> Ipv4 {
    let mut out = ip.clone();
    if outbound {
        out.src = addr;
    } else {
        out.dst = addr;
    }
    match ip.protocol {
        PROTO_ICMP => {
            if let (Some(new), Ok(icmp)) = (id, Icmp::decode(&ip.payload)) {
                let icmp = match icmp {
                    Icmp::EchoRequest { seq, data, .. } => Icmp::EchoRequest { id: new, seq, data },
                    Icmp::EchoReply { seq, data, .. } => Icmp::EchoReply { id: new, seq, data },
                    other => other,
                };
                out.payload = icmp.encode();
            }
        }
        PROTO_UDP => {
            if let Ok(mut u) = Udp::decode(&ip.payload, ip.src, ip.dst) {
                if let Some(new) = id {
                    if outbound {
                        u.src_port = new;
                    } else {
                        u.dst_port = new;
                    }
                }
                out.payload = u.encode(out.src, out.dst);
            }
        }
        PROTO_TCP => {
            if let Ok(mut u) = Tcp::decode(&ip.payload, ip.src, ip.dst) {
                if let Some(new) = id {
                    if outbound {
                        u.src_port = new;
                    } else {
                        u.dst_port = new;
                    }
                }
                out.payload = u.encode(out.src, out.dst);
            }
        }
        _ => {}
    }
    out
}
