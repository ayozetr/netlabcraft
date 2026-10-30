//! Control-plane traffic on the wire: spanning-tree BPDUs, OSPF and EIGRP hellos, RIP updates, HSRP
//! hellos, NTP polls, LLDP advertisements and LACPDUs.
//!
//! The protocols themselves are computed from the topology (`stp`, `ospf`, `rip`); these frames
//! show what they would say, at their usual intervals, so Simulation mode and captures have them.
//! Receivers take them in quietly.

use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr, VlanId};
use netlabcraft_model::{DeviceKind, End, Topology};
use netlabcraft_wire::control::{self, Bpdu, BridgeId, OspfHello, RipEntry};
use netlabcraft_wire::ospfdb::{DBD_INIT, DBD_MASTER, DBD_MORE, OspfDb, RouterLink, RouterLsa};
use netlabcraft_wire::{Frame, Ipv4, PROTO_UDP, Udp};

use crate::{LINK_DELAY_US, Sim, l3_ifaces, port_up};

/// Hello time of spanning tree.
pub const BPDU_EVERY_US: u64 = 2_000_000;
pub const OSPF_HELLO_EVERY_US: u64 = 10_000_000;
pub const RIP_UPDATE_EVERY_US: u64 = 30_000_000;
/// EIGRP hellos are considered every second; each interface sends at its own hello interval.
pub const EIGRP_HELLO_EVERY_US: u64 = 1_000_000;
pub const HSRP_HELLO_EVERY_US: u64 = 3_000_000;
/// NTP clients poll every 64 s.
pub const NTP_POLL_EVERY_US: u64 = 64_000_000;
/// A long run only replays the last ten minutes of control traffic.
const MAX_CATCH_UP_US: u64 = 600_000_000;

/// A port id on the wire: the port's priority in the top four bits (IEEE 802.1D-2004 9.2.7)
/// and its number below.
fn wire_port_id(port_id: u32, port: usize) -> u16 {
    (u16::try_from(port_id / 1000).unwrap_or(128).min(240) << 8) | (u16::try_from(port + 1).unwrap_or(0xff) & 0x0fff)
}

impl Sim {
    /// Queues the control frames due in `(self.ctl_until, deadline]`.
    pub(crate) fn schedule_control(&mut self, t: &Topology, deadline: u64) {
        let from = self.ctl_until.max(deadline.saturating_sub(MAX_CATCH_UP_US));
        if deadline <= from {
            return;
        }
        self.ctl_until = deadline;
        for (every, kind) in [
            (BPDU_EVERY_US, 0u8),
            (OSPF_HELLO_EVERY_US, 1),
            (RIP_UPDATE_EVERY_US, 2),
            (HSRP_HELLO_EVERY_US, 3),
            (NTP_POLL_EVERY_US, 4),
            (crate::lldp::LLDP_EVERY_US, 5),
            (crate::channel::LACP_EVERY_US, 6),
            (OSPF_HELLO_EVERY_US, 7),
            (EIGRP_HELLO_EVERY_US, 8),
        ] {
            let mut at = (from / every + 1) * every;
            while at <= deadline {
                match kind {
                    0 => self.send_bpdus(t, at),
                    1 => self.send_ospf_hellos(t, at),
                    3 => self.send_hsrp_hellos(t, at),
                    4 => self.send_ntp_polls(t, at),
                    5 => self.send_lldp(t, at),
                    6 => self.send_lacp(t, at),
                    7 => self.send_ospf6_hellos(t, at),
                    8 => self.send_eigrp_hellos(t, at),
                    _ => self.send_rip_updates(t, at),
                }
                at += every;
            }
        }
    }

    fn queue_at(&mut self, t: &Topology, from: End, frame: Vec<u8>, at: u64) {
        if !port_up(t, from) {
            return;
        }
        let Some(to) = t.peer(from) else { return };
        self.push_hop(t, at + LINK_DELAY_US, from, to, frame, 0);
    }

    fn send_lldp(&mut self, t: &Topology, at: u64) {
        let mut frames = Vec::new();
        for d in t.devices.iter().filter(|d| d.lldp_on) {
            for (i, p) in d.ports.iter().enumerate() {
                if let Some(l) = crate::lldp::advertisement(d, i) {
                    let f = Frame {
                        dst: netlabcraft_wire::lldp::LLDP_GROUP,
                        src: p.mac,
                        vlan: None,
                        ethertype: netlabcraft_wire::lldp::ETHERTYPE_LLDP,
                        payload: l.encode(),
                    };
                    frames.push((End { device: d.id, port: i }, f.encode()));
                }
            }
        }
        for (from, f) in frames {
            self.queue_at(t, from, f, at);
        }
    }

    fn send_lacp(&mut self, t: &Topology, at: u64) {
        let mut frames = Vec::new();
        for d in t.devices.iter().filter(|d| d.kind == DeviceKind::Switch) {
            for (i, p) in d.ports.iter().enumerate().filter(|(_, p)| p.channel.is_some()) {
                if let Some(pdu) = crate::channel::lacpdu(t, d, i) {
                    let f = Frame {
                        dst: netlabcraft_wire::lacp::SLOW_GROUP,
                        src: p.mac,
                        vlan: None,
                        ethertype: netlabcraft_wire::lacp::ETHERTYPE_SLOW,
                        payload: pdu.encode(),
                    };
                    frames.push((End { device: d.id, port: i }, f.encode()));
                }
            }
        }
        for (from, f) in frames {
            self.queue_at(t, from, f, at);
        }
    }

    fn send_bpdus(&mut self, t: &Topology, at: u64) {
        let switches: Vec<_> = t.devices.iter().filter(|d| d.kind == DeviceKind::Switch).map(|d| d.id).collect();
        for id in switches {
            let Some(d) = t.device(id) else { continue };
            let vlans: Vec<VlanId> = d.vlans.keys().copied().collect();
            for v in vlans {
                let tree = self.spanning_tree(t, v).clone();
                let Some(me) = tree.bridges.get(&id) else { continue };
                let wire = |b: crate::stp::BridgeId| BridgeId { priority: u16::try_from(b.priority).unwrap_or(u16::MAX), mac: b.mac };
                for ((dev, p), info) in tree.ports.iter().filter(|((dev, _), i)| *dev == id && i.role == crate::stp::Role::Designated) {
                    let Some(port) = d.ports.get(*p) else { continue };
                    if crate::stp::bpdufilter_on(d, *p) {
                        continue;
                    }
                    let tag = (t.is_trunk(End { device: *dev, port: *p }) && port.switchport.native_vlan != v).then(|| v.get());
                    // Rapid spanning tree sends RST BPDUs: a designated port, learning and
                    // forwarding as its state says.
                    let rstp = d.rapid_stp.then(|| {
                        let state = match self.stp_transition(id, *p, v) {
                            Some("LIS") => 0,
                            Some(_) => control::FLAG_LEARNING,
                            None => control::FLAG_LEARNING | control::FLAG_FORWARDING,
                        };
                        (control::ROLE_DESIGNATED << control::ROLE_SHIFT) | state
                    });
                    let bpdu = Bpdu {
                        root: wire(me.root),
                        root_cost: me.root_cost,
                        bridge: wire(me.id),
                        port_id: wire_port_id(info.port_id, *p),
                        message_age: u16::from(me.root != me.id),
                        rstp,
                    };
                    let frame = Frame { dst: control::STP_GROUP, src: port.mac, vlan: tag, ethertype: bpdu.length(), payload: bpdu.encode() };
                    self.queue_at(t, End { device: *dev, port: *p }, frame.encode(), at);
                }
            }
        }
    }

    /// Rapid spanning tree (802.1w): a designated port that just came up on a point-to-point
    /// link proposes (an RST BPDU with the proposal flag, discarding), and the switch at the
    /// far end, having put its other ports in sync, agrees from its root (or alternate) port.
    /// Both ports then forward at once instead of waiting out the timers.
    pub(crate) fn send_rstp_handshakes(&mut self, t: &Topology) {
        if !t.devices.iter().any(|d| d.rapid_stp) {
            self.rstp_handshakes.clear();
            return;
        }
        // Work out every rapid switch's trees so the ports that just changed are known.
        let vlans: std::collections::BTreeSet<VlanId> =
            t.devices.iter().filter(|d| d.kind == DeviceKind::Switch && d.rapid_stp).flat_map(|d| d.vlans.keys().copied()).collect();
        for v in &vlans {
            let _ = self.spanning_tree(t, *v);
        }
        let now = self.now;
        for (v, dev, p) in std::mem::take(&mut self.rstp_handshakes) {
            let Some(vlan) = VlanId::new(v) else { continue };
            let tree = self.spanning_tree(t, vlan).clone();
            let (Some(d), Some(me)) = (t.device(dev), tree.bridges.get(&dev)) else { continue };
            let Some(info) = tree.ports.get(&(dev, p)) else { continue };
            let Some(port) = d.ports.get(p) else { continue };
            let end = End { device: dev, port: p };
            let Some(far) = t.peer(end) else { continue };
            let Some(fd) = t.device(far.device) else { continue };
            let Some(fport) = fd.ports.get(far.port) else { continue };
            let wire = |b: crate::stp::BridgeId| BridgeId { priority: u16::try_from(b.priority).unwrap_or(u16::MAX), mac: b.mac };
            let tag_at = |dd: &netlabcraft_model::Device, e: End| {
                let native = dd.ports.get(e.port).map(|x| x.switchport.native_vlan);
                (t.is_trunk(e) && native != Some(vlan)).then_some(v)
            };
            let proposal = Bpdu {
                root: wire(me.root),
                root_cost: me.root_cost,
                bridge: wire(me.id),
                port_id: wire_port_id(info.port_id, p),
                message_age: u16::from(me.root != me.id),
                rstp: Some((control::ROLE_DESIGNATED << control::ROLE_SHIFT) | control::FLAG_PROPOSAL),
            };
            let f = Frame { dst: control::STP_GROUP, src: port.mac, vlan: tag_at(d, end), ethertype: proposal.length(), payload: proposal.encode() };
            self.queue_at(t, end, f.encode(), now);
            // The agreement echoes the proposal's priority vector, from the far port's role.
            let frole = tree.ports.get(&(far.device, far.port)).map_or(crate::stp::Role::Root, |i| i.role);
            let role_bits = match frole {
                crate::stp::Role::Root => (control::ROLE_ROOT << control::ROLE_SHIFT) | control::FLAG_LEARNING | control::FLAG_FORWARDING,
                _ => control::ROLE_ALTERNATE << control::ROLE_SHIFT,
            };
            let agreement = Bpdu { rstp: Some(role_bits | control::FLAG_AGREEMENT), ..proposal };
            let f =
                Frame { dst: control::STP_GROUP, src: fport.mac, vlan: tag_at(fd, far), ethertype: agreement.length(), payload: agreement.encode() };
            self.queue_at(t, far, f.encode(), now + LINK_DELAY_US);
        }
    }

    fn send_ospf_hellos(&mut self, t: &Topology, at: u64) {
        if !t.devices.iter().any(|d| d.ospf.is_some()) {
            return;
        }
        let st = self.ospf_state(t);
        for (id, ifs) in &st.interfaces {
            let (Some(d), Some(rid)) = (t.device(*id), st.router_ids.get(id)) else { continue };
            for (name, ..) in ifs.iter().filter(|x| !x.3) {
                let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == *name) else { continue };
                let heard: Vec<Ipv4Addr> = st.neighbors.get(id).into_iter().flatten().filter(|n| n.iface == *name).map(|n| n.router_id).collect();
                let seg = st.segments.get(&(*id, name.clone())).copied();
                let area = seg.map_or(0, |s| s.area);
                let (hello_secs, dead) = crate::ospf::timers(d, name);
                // DR and BDR are only announced once there is someone to elect with.
                let elected = |a: Option<Ipv4Addr>| a.filter(|_| !heard.is_empty()).unwrap_or(Ipv4Addr::UNSPECIFIED);
                let hello = OspfHello {
                    router_id: *rid,
                    area,
                    mask: ifc.net.mask(),
                    hello_secs,
                    dead_secs: u32::from(dead),
                    priority: seg.map_or(1, |s| s.priority),
                    dr: elected(seg.and_then(|s| s.dr)),
                    bdr: elected(seg.and_then(|s| s.bdr)),
                    neighbors: heard.clone(),
                };
                let ip =
                    Ipv4 { id: 0, ttl: 1, protocol: control::PROTO_OSPF, src: ifc.net.addr, dst: control::OSPF_ALL_ROUTERS, payload: hello.encode() };
                self.queue_l3(t, d, &ifc, ifc.mac, control::ipv4_multicast_mac(control::OSPF_ALL_ROUTERS), ip.encode(), at);
            }
        }
        self.ospf_exchanges(t, &st, at);
    }

    /// A router-LSA as the computed state describes the router in one area: a stub link per
    /// OSPF network, a point-to-point link per neighbour.
    fn router_lsa(st: &crate::ospf::OspfState, id: DeviceId, area: u32) -> Option<RouterLsa> {
        let rid = *st.router_ids.get(&id)?;
        let mut links = Vec::new();
        for (name, net, cost, _) in st.interfaces.get(&id).into_iter().flatten() {
            if st.segments.get(&(id, name.clone())).is_some_and(|s| s.area != area) {
                continue;
            }
            let metric = u16::try_from(*cost).unwrap_or(u16::MAX);
            for n in st.neighbors.get(&id).into_iter().flatten().filter(|n| n.iface == *name) {
                links.push(RouterLink { kind: 1, id: n.router_id, data: net.addr, metric });
            }
            links.push(RouterLink { kind: 3, id: net.network().addr, data: net.mask(), metric });
        }
        Some(RouterLsa { router_id: rid, seq: 0x8000_0001, links })
    }

    /// Database exchange for adjacencies that weren't there at the last hello: each side sends
    /// its first description, its LSA headers, a request for the neighbour's router-LSA, an
    /// update with its own, and an acknowledgement.
    fn ospf_exchanges(&mut self, t: &Topology, st: &crate::ospf::OspfState, at: u64) {
        let now: std::collections::BTreeSet<(DeviceId, String, Ipv4Addr)> =
            st.neighbors.iter().flat_map(|(id, ns)| ns.iter().map(|n| (*id, n.iface.clone(), n.router_id))).collect();
        let fresh: Vec<_> = now.difference(&self.ospf_adjacent).cloned().collect();
        self.ospf_adjacent = now;
        for (id, iface, nbr) in fresh {
            let (Some(d), Some(rid)) = (t.device(id), st.router_ids.get(&id).copied()) else { continue };
            let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == iface) else { continue };
            let area = st.segments.get(&(id, iface.clone())).map_or(0, |s| s.area);
            let in_area: Vec<DeviceId> =
                st.router_ids.keys().copied().filter(|r| st.segments.iter().any(|((dev, _), s)| dev == r && s.area == area)).collect();
            let headers: Vec<_> = in_area.iter().filter_map(|r| Self::router_lsa(st, *r, area)).map(|l| l.header()).collect();
            let theirs = st.router_ids.iter().find(|(_, r)| **r == nbr).and_then(|(dev, _)| Self::router_lsa(st, *dev, area));
            let master = if rid > nbr { DBD_MASTER } else { 0 };
            let seq = u32::from(rid) & 0xffff;
            let mut packets = vec![
                OspfDb::Dbd { flags: DBD_INIT | DBD_MORE | DBD_MASTER, seq, headers: Vec::new() },
                OspfDb::Dbd { flags: master, seq: seq + 1, headers },
                OspfDb::Request(vec![(1, nbr, nbr)]),
            ];
            if let Some(mine) = Self::router_lsa(st, id, area) {
                packets.push(OspfDb::Update(vec![mine]));
            }
            if let Some(l) = theirs {
                packets.push(OspfDb::Ack(vec![l.header()]));
            }
            for (k, p) in (1u64..).zip(packets) {
                let ip = Ipv4 {
                    id: 0,
                    ttl: 1,
                    protocol: control::PROTO_OSPF,
                    src: ifc.net.addr,
                    dst: control::OSPF_ALL_ROUTERS,
                    payload: p.encode(rid, area),
                };
                // A millisecond apart, so the event list shows them in order.
                self.queue_l3(t, d, &ifc, ifc.mac, control::ipv4_multicast_mac(control::OSPF_ALL_ROUTERS), ip.encode(), at + k * 1_000);
            }
        }
    }

    /// EIGRP hellos (RFC 7868) from every EIGRP interface that isn't passive, at the interface's
    /// hello interval, announcing its hold time.
    fn send_eigrp_hellos(&mut self, t: &Topology, at: u64) {
        use netlabcraft_wire::eigrp::{EIGRP_ROUTERS, EigrpHello, PROTO_EIGRP};
        if !t.devices.iter().any(|d| d.eigrp.is_some()) {
            return;
        }
        self.eigrp_refresh(t);
        let due: Vec<(DeviceId, String, u16)> = self
            .eigrp
            .as_ref()
            .map(|st| {
                st.interfaces
                    .iter()
                    .flat_map(|(id, ifs)| {
                        ifs.iter()
                            .filter(|x| !x.passive && at.is_multiple_of(u64::from(x.hello_secs.max(1)) * 1_000_000))
                            .map(|x| (*id, x.name.clone(), x.hold_secs))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (id, name, hold) in due {
            let Some(d) = t.device(id) else { continue };
            let Some(e) = &d.eigrp else { continue };
            let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == name) else { continue };
            let ip = Ipv4 {
                id: 0,
                ttl: 2,
                protocol: PROTO_EIGRP,
                src: ifc.net.addr,
                dst: EIGRP_ROUTERS,
                payload: EigrpHello::with_hold(e.asn, hold).encode(),
            };
            self.queue_l3(t, d, &ifc, ifc.mac, control::ipv4_multicast_mac(EIGRP_ROUTERS), ip.encode(), at);
        }
    }

    fn send_rip_updates(&mut self, t: &Topology, at: u64) {
        if !t.devices.iter().any(|d| d.rip.is_some()) {
            return;
        }
        let st = self.rip_state(t);
        for (id, ifs) in &st.interfaces {
            let Some(d) = t.device(*id) else { continue };
            let Some(r) = &d.rip else { continue };
            for (name, ..) in ifs.iter().filter(|x| !x.2) {
                let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == *name) else { continue };
                // Split horizon: nothing learned through, or attached to, this interface.
                let mut entries: Vec<RipEntry> =
                    ifs.iter().filter(|(n, ..)| n != name).map(|(_, other, _)| RipEntry { net: other.addr, mask: other.mask(), metric: 1 }).collect();
                for route in st.routes.get(id).into_iter().flatten().filter(|x| x.iface != *name) {
                    entries.push(RipEntry { net: route.net.addr, mask: route.net.mask(), metric: u32::from(route.hops) + 1 });
                }
                let dst = if r.version >= 2 { control::RIP_ROUTERS } else { Ipv4Addr::BROADCAST };
                let mac = if r.version >= 2 { control::ipv4_multicast_mac(dst) } else { MacAddr::BROADCAST };
                for chunk in entries.chunks(control::RIP_MAX_ENTRIES) {
                    let udp = Udp { src_port: control::RIP_PORT, dst_port: control::RIP_PORT, payload: control::rip_encode(r.version, chunk) };
                    let ip = Ipv4 { id: 0, ttl: 1, protocol: PROTO_UDP, src: ifc.net.addr, dst, payload: udp.encode(ifc.net.addr, dst) };
                    self.queue_l3(t, d, &ifc, ifc.mac, mac, ip.encode(), at);
                }
            }
        }
    }

    fn send_ntp_polls(&mut self, t: &Topology, at: u64) {
        let clients: Vec<_> = t.devices.iter().filter(|d| !d.ntp_servers.is_empty()).map(|d| d.id).collect();
        for id in clients {
            let Some(d) = t.device(id) else { continue };
            for server in d.ntp_servers.clone() {
                let Some((ifc, _)) = crate::route(t, d, server) else { continue };
                let req = netlabcraft_wire::mgmt::Ntp { server: false, stratum: 0, time: self.clock(id).0 };
                let udp = Udp { src_port: netlabcraft_wire::mgmt::NTP_PORT, dst_port: netlabcraft_wire::mgmt::NTP_PORT, payload: req.encode() };
                let ip = Ipv4 { id: 0, ttl: 255, protocol: PROTO_UDP, src: ifc.net.addr, dst: server, payload: udp.encode(ifc.net.addr, server) };
                // Only the first hop needs a known address: polls go out where ARP already knows the way.
                let Some((_, nh)) = crate::route(t, d, server) else { continue };
                let Some(mac) = self.state.get(&id).and_then(|s| s.arp.get(&nh)).map(|e| e.mac) else { continue };
                self.queue_l3(t, d, &ifc, ifc.mac, mac, ip.encode(), at);
            }
        }
    }

    fn send_hsrp_hellos(&mut self, t: &Topology, at: u64) {
        let roles = self.hsrp_roles(t);
        for (id, list) in roles {
            let Some(d) = t.device(id) else { continue };
            for r in list.iter().filter(|r| r.state != crate::hsrp::State::Listen) {
                let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == r.iface) else { continue };
                let active = r.state == crate::hsrp::State::Active;
                let hello = control::HsrpHello { active, priority: r.priority, group: r.group, virtual_ip: r.virtual_ip };
                let udp = Udp { src_port: control::HSRP_PORT, dst_port: control::HSRP_PORT, payload: hello.encode() };
                let group = if r.version == 2 { crate::hsrp::HSRP2_GROUP } else { control::ALL_ROUTERS };
                let ip = Ipv4 { id: 0, ttl: 1, protocol: PROTO_UDP, src: ifc.net.addr, dst: group, payload: udp.encode(ifc.net.addr, group) };
                // The active router speaks with the virtual MAC, so switches learn where it is.
                let src = if active { r.virtual_mac() } else { ifc.mac };
                self.queue_l3(t, d, &ifc, src, control::ipv4_multicast_mac(group), ip.encode(), at);
            }
        }
    }

    /// Queues an IPv4 packet out of a layer-3 interface at a given time.
    pub(crate) fn queue_l3(
        &mut self,
        t: &Topology,
        d: &netlabcraft_model::Device,
        ifc: &crate::L3If,
        src: MacAddr,
        dst: MacAddr,
        packet: Vec<u8>,
        at: u64,
    ) {
        let frame = Frame { dst, src, vlan: ifc.vlan, ethertype: netlabcraft_wire::ETHERTYPE_IPV4, payload: packet };
        self.queue_frame(t, d, ifc.port, frame, at);
    }

    /// OSPFv3 hellos from every interface of every OSPFv3 router (passive ones excepted).
    fn send_ospf6_hellos(&mut self, t: &Topology, at: u64) {
        use netlabcraft_wire::ipv6::{ETHERTYPE_IPV6, Ipv6, multicast_mac};
        use netlabcraft_wire::ospf6::{ALL_SPF_ROUTERS6, Hello6, PROTO_OSPF};
        if !t.devices.iter().any(|d| d.ospf6.is_some()) {
            return;
        }
        let st = self.ospf6_state(t);
        for (id, ifs) in &st.interfaces {
            let (Some(d), Some(rid)) = (t.device(*id), st.router_ids.get(id)) else { continue };
            let up = crate::v6::up_ifaces6(t, d);
            for (k, (name, area, _, _)) in ifs.iter().enumerate().filter(|(_, x)| !x.3) {
                let Some(ifc) = up.iter().find(|i| i.name == *name) else { continue };
                let heard: Vec<Ipv4Addr> = st.neighbors.get(id).into_iter().flatten().filter(|n| n.iface == *name).map(|n| n.router_id).collect();
                let hello = Hello6 {
                    router_id: *rid,
                    area: *area,
                    interface_id: u32::try_from(k + 1).unwrap_or(0),
                    priority: 1,
                    dr: Ipv4Addr::UNSPECIFIED,
                    bdr: Ipv4Addr::UNSPECIFIED,
                    neighbors: heard,
                };
                let ip = Ipv6 {
                    hop_limit: 1,
                    next_header: PROTO_OSPF,
                    src: ifc.link_local,
                    dst: ALL_SPF_ROUTERS6,
                    payload: hello.encode(ifc.link_local, ALL_SPF_ROUTERS6),
                };
                let frame =
                    Frame { dst: multicast_mac(ALL_SPF_ROUTERS6), src: ifc.mac, vlan: ifc.vlan, ethertype: ETHERTYPE_IPV6, payload: ip.encode() };
                self.queue_frame(t, d, ifc.port, frame, at);
            }
        }
    }

    /// Queues a frame out of a port at a given time (re-framed on serial); a VLAN interface's
    /// frame (`SVI_PORT`, its VLAN in the tag) goes through the switch at once.
    fn queue_frame(&mut self, t: &Topology, d: &netlabcraft_model::Device, port: usize, frame: Frame, at: u64) {
        if crate::vpn::is_tunnel(port) {
            // Into a GRE tunnel, at once: encapsulated and routed to the far end.
            if frame.ethertype == netlabcraft_wire::ETHERTYPE_IPV4
                && let Some(ifc) = crate::up_ifaces(t, d).into_iter().find(|i| i.port == port)
            {
                let _ = self.gre_out(t, d, &ifc, frame.payload);
            }
            return;
        }
        if port == crate::SVI_PORT {
            // A VLAN interface sends through its own switch, at once (not at `at`).
            if let Some(v) = frame.vlan.and_then(VlanId::new) {
                let _ = self.switch_forward(t, d, None, v, frame.retagged(None), 0);
            }
            return;
        }
        let end = End { device: d.id, port };
        let bytes = match d.ports.get(port).filter(|p| netlabcraft_model::ifname::is_serial(&p.name)) {
            Some(p) => match netlabcraft_wire::serial::encode(crate::framing(p), frame.ethertype, &frame.payload) {
                Ok(b) => b,
                Err(_) => return,
            },
            None => frame.encode(),
        };
        self.queue_at(t, end, bytes, at);
    }
}
