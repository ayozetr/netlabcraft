//! Control-plane traffic on the wire: spanning-tree BPDUs, OSPF hellos and RIP updates.
//!
//! The protocols themselves are computed from the topology (`stp`, `ospf`, `rip`); these frames
//! show what they would say, at their usual intervals, so Simulation mode and captures have them.
//! Receivers take them in quietly.

use std::net::Ipv4Addr;

use packetcraft_core::{MacAddr, VlanId};
use packetcraft_model::{DeviceKind, End, Topology};
use packetcraft_wire::control::{self, Bpdu, BridgeId, OspfHello, RipEntry};
use packetcraft_wire::{Frame, Ipv4, PROTO_UDP, Udp};

use crate::{LINK_DELAY_US, Queued, Sim, l3_ifaces, port_up};

/// Hello time of spanning tree.
pub const BPDU_EVERY_US: u64 = 2_000_000;
pub const OSPF_HELLO_EVERY_US: u64 = 10_000_000;
pub const RIP_UPDATE_EVERY_US: u64 = 30_000_000;
pub const HSRP_HELLO_EVERY_US: u64 = 3_000_000;
/// A long run only replays the last ten minutes of control traffic.
const MAX_CATCH_UP_US: u64 = 600_000_000;

impl Sim {
    /// Queues the control frames due in `(self.ctl_until, deadline]`.
    pub(crate) fn schedule_control(&mut self, t: &Topology, deadline: u64) {
        let from = self.ctl_until.max(deadline.saturating_sub(MAX_CATCH_UP_US));
        if deadline <= from {
            return;
        }
        self.ctl_until = deadline;
        for (every, kind) in [(BPDU_EVERY_US, 0u8), (OSPF_HELLO_EVERY_US, 1), (RIP_UPDATE_EVERY_US, 2), (HSRP_HELLO_EVERY_US, 3)] {
            let mut at = (from / every + 1) * every;
            while at <= deadline {
                match kind {
                    0 => self.send_bpdus(t, at),
                    1 => self.send_ospf_hellos(t, at),
                    3 => self.send_hsrp_hellos(t, at),
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
        self.seq += 1;
        self.queue.push(std::cmp::Reverse(Queued { at: at + LINK_DELAY_US, seq: self.seq, from, to, frame, hops: 0 }));
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
                for ((dev, p), _) in tree.ports.iter().filter(|((dev, _), i)| *dev == id && i.role == crate::stp::Role::Designated) {
                    let Some(port) = d.ports.get(*p) else { continue };
                    let tag = (t.is_trunk(End { device: *dev, port: *p }) && port.switchport.native_vlan != v).then(|| v.get());
                    let bpdu = Bpdu {
                        root: wire(me.root),
                        root_cost: me.root_cost,
                        bridge: wire(me.id),
                        port_id: 0x8000 | u16::try_from(p + 1).unwrap_or(0xff),
                        message_age: u16::from(me.root != me.id),
                    };
                    let frame = Frame { dst: control::STP_GROUP, src: port.mac, vlan: tag, ethertype: Bpdu::frame_length(), payload: bpdu.encode() };
                    self.queue_at(t, End { device: *dev, port: *p }, frame.encode(), at);
                }
            }
        }
    }

    fn send_ospf_hellos(&mut self, t: &Topology, at: u64) {
        if !t.devices.iter().any(|d| d.ospf.is_some()) {
            return;
        }
        let st = self.ospf_state(t);
        for (id, ifs) in &st.interfaces {
            let (Some(d), Some(rid)) = (t.device(*id), st.router_ids.get(id)) else { continue };
            let Some(o) = &d.ospf else { continue };
            for (name, ..) in ifs.iter().filter(|x| !x.3) {
                let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == *name) else { continue };
                let heard: Vec<(Ipv4Addr, Ipv4Addr)> =
                    st.neighbors.get(id).into_iter().flatten().filter(|n| n.iface == *name).map(|n| (n.router_id, n.address)).collect();
                // DR and BDR: highest router ids on the segment (all priorities 1).
                let mut seg: Vec<(Ipv4Addr, Ipv4Addr)> = heard.clone();
                seg.push((*rid, ifc.net.addr));
                seg.sort_by_key(|x| std::cmp::Reverse(x.0));
                let addr = |i: usize| seg.get(i).filter(|_| seg.len() > 1).map_or(Ipv4Addr::UNSPECIFIED, |x| x.1);
                let area = o.networks.iter().find(|n| n.matches(ifc.net.addr)).map_or(0, |n| n.area);
                let hello = OspfHello {
                    router_id: *rid,
                    area,
                    mask: ifc.net.mask(),
                    priority: 1,
                    dr: addr(0),
                    bdr: addr(1),
                    neighbors: heard.iter().map(|h| h.0).collect(),
                };
                let ip =
                    Ipv4 { id: 0, ttl: 1, protocol: control::PROTO_OSPF, src: ifc.net.addr, dst: control::OSPF_ALL_ROUTERS, payload: hello.encode() };
                self.queue_l3(t, d, &ifc, ifc.mac, control::ipv4_multicast_mac(control::OSPF_ALL_ROUTERS), ip.encode(), at);
            }
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

    fn send_hsrp_hellos(&mut self, t: &Topology, at: u64) {
        let roles = self.hsrp_roles(t);
        for (id, list) in roles {
            let Some(d) = t.device(id) else { continue };
            for r in list.iter().filter(|r| r.state != crate::hsrp::State::Listen) {
                let Some(ifc) = l3_ifaces(d).into_iter().find(|i| i.name == r.iface) else { continue };
                let active = r.state == crate::hsrp::State::Active;
                let hello = control::HsrpHello { active, priority: r.priority, group: r.group, virtual_ip: r.virtual_ip };
                let udp = Udp { src_port: control::HSRP_PORT, dst_port: control::HSRP_PORT, payload: hello.encode() };
                let ip = Ipv4 {
                    id: 0,
                    ttl: 1,
                    protocol: PROTO_UDP,
                    src: ifc.net.addr,
                    dst: control::ALL_ROUTERS,
                    payload: udp.encode(ifc.net.addr, control::ALL_ROUTERS),
                };
                // The active router speaks with the virtual MAC, so switches learn where it is.
                let src = if active { r.virtual_mac() } else { ifc.mac };
                self.queue_l3(t, d, &ifc, src, control::ipv4_multicast_mac(control::ALL_ROUTERS), ip.encode(), at);
            }
        }
    }

    /// Queues an IPv4 packet out of a layer-3 interface at a given time.
    fn queue_l3(&mut self, t: &Topology, d: &packetcraft_model::Device, ifc: &crate::L3If, src: MacAddr, dst: MacAddr, packet: Vec<u8>, at: u64) {
        let frame = Frame { dst, src, vlan: ifc.vlan, ethertype: packetcraft_wire::ETHERTYPE_IPV4, payload: packet };
        if ifc.port == crate::SVI_PORT {
            // A VLAN interface sends through its own switch, at once (not at `at`).
            if let Some(v) = ifc.vlan.and_then(VlanId::new) {
                let _ = self.switch_forward(t, d, None, v, frame, 0);
            }
            return;
        }
        let end = End { device: d.id, port: ifc.port };
        let bytes = match d.ports.get(ifc.port).filter(|p| packetcraft_model::ifname::is_serial(&p.name)) {
            Some(p) => match packetcraft_wire::serial::encode(crate::framing(p), frame.ethertype, &frame.payload) {
                Ok(b) => b,
                Err(_) => return,
            },
            None => frame.encode(),
        };
        self.queue_at(t, end, bytes, at);
    }
}
