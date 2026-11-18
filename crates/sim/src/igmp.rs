//! IGMP snooping and the IGMP behaviour of hosts.
//!
//! A snooping switch listens to the IGMP messages that cross it: a general query marks the port
//! it came in on as one that leads to a multicast router (a `mrouter` port), a membership report
//! marks the port as a member of the group, and a leave removes it. IPv4 multicast data is then
//! forwarded only out of member ports and `mrouter` ports; protocol traffic for `224.0.0.0/24`
//! and frames in VLANs where snooping is off flood as before. Reports and leaves are forwarded to
//! `mrouter` ports only. With `ip igmp snooping querier` the switch sends a general query every
//! minute from its VLAN interface address.
//!
//! A host that has joined a group (`igmp join`) sends a report, answers every query with a
//! report per group, and takes multicast datagrams for its groups. Hosts have no report delay or
//! suppression: every member answers at once.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr, VlanId};
use netlabcraft_model::{Device, DeviceKind, End, Topology};
use netlabcraft_wire::control::ipv4_multicast_mac;
use netlabcraft_wire::igmp::{ALL_HOSTS, ALL_ROUTERS, Igmp, IgmpKind, PROTO_IGMP};
use netlabcraft_wire::{ETHERTYPE_IPV4, Frame, Ipv4};

use crate::{L3If, Sim, l3_ifaces, port_up, up_ifaces};

/// How long a port stays a member of a group after its last report.
pub const MEMBER_US: u64 = 260_000_000;
/// How long a port stays an `mrouter` port after its last query.
pub const MROUTER_US: u64 = 300_000_000;
/// The querier asks this often.
pub const QUERY_EVERY_US: u64 = 60_000_000;
/// Most groups a switch tracks.
pub const MAX_GROUPS: usize = 1_024;

/// What the snooping switches have learned.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// (switch, VLAN, group) to the member ports and when each stops being one.
    groups: BTreeMap<(DeviceId, u16, Ipv4Addr), BTreeMap<usize, u64>>,
    /// (switch, VLAN, port) to when the port stops being an `mrouter` port.
    mrouters: BTreeMap<(DeviceId, u16, usize), u64>,
    /// Queries each switch sent (for `show ip igmp snooping querier`).
    queries: BTreeMap<(DeviceId, u16), (Ipv4Addr, u64)>,
}

/// One row of `show ip igmp snooping groups`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    pub vlan: u16,
    pub group: Ipv4Addr,
    pub ports: Vec<usize>,
}

/// The IGMP message in a frame, if it carries one.
fn message(f: &Frame) -> Option<(Ipv4, Igmp)> {
    if f.ethertype != ETHERTYPE_IPV4 {
        return None;
    }
    let ip = Ipv4::decode(&f.payload).ok()?;
    if ip.protocol != PROTO_IGMP {
        return None;
    }
    let m = Igmp::decode(&ip.payload).ok()?;
    Some((ip, m))
}

impl Sim {
    /// What a snooping switch learns from a frame arriving on `in_port`.
    pub(crate) fn igmp_learn(&mut self, t: &Topology, d: &Device, in_port: usize, vlan: VlanId, f: &Frame) {
        if !d.igmp.active(vlan.get()) {
            return;
        }
        let Some((ip, m)) = message(f) else { return };
        let port = self.chmap(t, d).logical(in_port);
        let now = self.now;
        let st = &mut self.igmp;
        match m.kind {
            IgmpKind::Query => {
                // A query from 0.0.0.0 is a switch probing, not a router.
                if !ip.src.is_unspecified() {
                    st.mrouters.insert((d.id, vlan.get(), port), now.saturating_add(MROUTER_US));
                }
            }
            IgmpKind::ReportV1 | IgmpKind::ReportV2 => {
                let key = (d.id, vlan.get(), m.group);
                if netlabcraft_model::igmp::Igmp::joinable(m.group) && (st.groups.len() < MAX_GROUPS || st.groups.contains_key(&key)) {
                    st.groups.entry(key).or_default().insert(port, now.saturating_add(MEMBER_US));
                }
            }
            IgmpKind::Leave => {
                let key = (d.id, vlan.get(), m.group);
                if let Some(ports) = st.groups.get_mut(&key) {
                    ports.remove(&port);
                    if ports.is_empty() {
                        st.groups.remove(&key);
                    }
                }
            }
        }
    }

    /// The ports a multicast frame may leave a snooping switch through: `None` when the frame is
    /// not subject to snooping (it floods as usual).
    pub(crate) fn igmp_outs(&self, d: &Device, vlan: VlanId, f: &Frame) -> Option<BTreeSet<usize>> {
        if !d.igmp.active(vlan.get()) || f.ethertype != ETHERTYPE_IPV4 || f.dst.0.get(..3) != Some(&[0x01, 0x00, 0x5e][..]) {
            return None;
        }
        let ip = Ipv4::decode(&f.payload).ok()?;
        if ip.dst.octets().get(..3) == Some(&[224, 0, 0][..]) {
            return None;
        }
        let now = self.now;
        let mut ports: BTreeSet<usize> = self
            .igmp
            .mrouters
            .iter()
            .filter(|((dev, v, _), until)| *dev == d.id && *v == vlan.get() && **until > now)
            .map(|((_, _, p), _)| *p)
            .collect();
        // Reports and leaves go to the routers only; data goes to the members as well.
        if ip.protocol != PROTO_IGMP
            && let Some(members) = self.igmp.groups.get(&(d.id, vlan.get(), ip.dst))
        {
            ports.extend(members.iter().filter(|(_, until)| **until > now).map(|(p, _)| *p));
        }
        Some(ports)
    }

    /// The groups a switch has learned, with their member ports.
    pub fn igmp_groups(&self, d: DeviceId) -> Vec<GroupRow> {
        let now = self.now;
        self.igmp
            .groups
            .iter()
            .filter(|((dev, ..), _)| *dev == d)
            .map(|((_, vlan, group), ports)| GroupRow {
                vlan: *vlan,
                group: *group,
                ports: ports.iter().filter(|(_, u)| **u > now).map(|(p, _)| *p).collect(),
            })
            .filter(|r| !r.ports.is_empty())
            .collect()
    }

    /// The ports of a switch that lead to a multicast router: (VLAN, port).
    pub fn igmp_mrouters(&self, d: DeviceId) -> Vec<(u16, usize)> {
        let now = self.now;
        self.igmp.mrouters.iter().filter(|((dev, _, _), until)| *dev == d && **until > now).map(|((_, v, p), _)| (*v, *p)).collect()
    }

    /// The queries a switch sent: VLAN, source address and when the last one went.
    pub fn igmp_queries(&self, d: DeviceId) -> Vec<(u16, Ipv4Addr, u64)> {
        self.igmp.queries.iter().filter(|((dev, _), _)| *dev == d).map(|((_, v), (a, at))| (*v, *a, *at)).collect()
    }

    /// Forgets a switch's learned state (`clear ip igmp snooping`).
    pub fn clear_igmp(&mut self, d: DeviceId) {
        self.igmp.groups.retain(|k, _| k.0 != d);
        self.igmp.mrouters.retain(|k, _| k.0 != d);
    }

    /// Every switch whose querier is on sends a general query into each snooped VLAN.
    pub(crate) fn send_igmp_queries(&mut self, t: &Topology, at: u64) {
        let mut sends: Vec<(End, Vec<u8>)> = Vec::new();
        let mut noted: Vec<(DeviceId, u16, Ipv4Addr)> = Vec::new();
        for d in t.devices.iter().filter(|d| d.kind == DeviceKind::Switch && d.igmp.on && d.igmp.querier) {
            for vlan in d.vlans.keys().copied().filter(|v| d.igmp.active(v.get())) {
                let own = l3_ifaces(d).into_iter().find(|i| i.vlan == Some(vlan.get())).map(|i| i.net.addr);
                let Some(src) = own.or(d.igmp.querier_address) else { continue };
                let ip = Ipv4 { id: 0, ttl: 1, protocol: PROTO_IGMP, src, dst: ALL_HOSTS, payload: Igmp::general_query(100).encode() };
                noted.push((d.id, vlan.get(), src));
                for (p, port) in d.ports.iter().enumerate() {
                    let end = End { device: d.id, port: p };
                    if !crate::stp::carries(t, d, p, vlan) || !port_up(t, end) {
                        continue;
                    }
                    let tag = if t.is_trunk(end) { (port.switchport.native_vlan != vlan).then_some(vlan.get()) } else { None };
                    let f = Frame { dst: ipv4_multicast_mac(ALL_HOSTS), src: port.mac, vlan: tag, ethertype: ETHERTYPE_IPV4, payload: ip.encode() };
                    sends.push((end, f.encode()));
                }
            }
        }
        for (dev, vlan, src) in noted {
            self.igmp.queries.insert((dev, vlan), (src, at));
        }
        for (end, bytes) in sends {
            self.queue_at(t, end, bytes, at);
        }
    }

    /// The interface a host sends IGMP and multicast data from: the first one that is up.
    pub(crate) fn host_multicast_iface(&self, t: &Topology, d: &Device) -> Option<L3If> {
        up_ifaces(t, d).into_iter().next().or_else(|| l3_ifaces(d).into_iter().next())
    }

    /// Sends a host's datagram to a multicast group out of its main interface, with the group's
    /// own Ethernet address (no gateway, no ARP).
    pub(crate) fn send_multicast(&mut self, t: &Topology, d: &Device, ip: &Ipv4) -> Result<(), String> {
        let ifc = self.host_multicast_iface(t, d).ok_or_else(|| format!("{} has no IP address configured", d.name))?;
        let f = Frame { dst: ipv4_multicast_mac(ip.dst), src: ifc.mac, vlan: ifc.vlan, ethertype: ETHERTYPE_IPV4, payload: ip.encode() };
        self.l3_emit(t, d, &ifc, f);
        Ok(())
    }

    fn igmp_send_from(&mut self, t: &Topology, d: &Device, ifc: &L3If, m: Igmp, dst: Ipv4Addr) {
        let ip = Ipv4 { id: self.ip_id(), ttl: 1, protocol: PROTO_IGMP, src: ifc.net.addr, dst, payload: m.encode() };
        let mac: MacAddr = ipv4_multicast_mac(dst);
        let f = Frame { dst: mac, src: ifc.mac, vlan: ifc.vlan, ethertype: ETHERTYPE_IPV4, payload: ip.encode() };
        self.l3_emit(t, d, ifc, f);
    }

    /// A host reports (or, with `leave`, gives up) a group it joined.
    pub fn igmp_announce(&mut self, t: &Topology, dev: DeviceId, group: Ipv4Addr, leave: bool) -> Result<(), String> {
        let d = t.device(dev).ok_or("no such device")?;
        let ifc = self.host_multicast_iface(t, d).ok_or_else(|| format!("{} has no IP address configured", d.name))?;
        if leave {
            self.igmp_send_from(t, d, &ifc, Igmp::leave(group), ALL_ROUTERS);
        } else {
            self.igmp_send_from(t, d, &ifc, Igmp::report(group), group);
        }
        let _ = self.advance(t, self.now.saturating_add(50_000), |_| false);
        Ok(())
    }

    /// A host takes a multicast datagram: IGMP queries (answered with a report per group), and
    /// data for a group it joined (or for all hosts). `None`: not for a host.
    pub(crate) fn host_multicast_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: Ipv4) -> Option<Result<(), String>> {
        if !ip.dst.is_multicast() || !d.kind.is_host() {
            return None;
        }
        if ip.protocol == PROTO_IGMP {
            let Ok(m) = Igmp::decode(&ip.payload) else { return Some(Err("malformed IGMP message".into())) };
            if m.kind == IgmpKind::Query {
                let groups: Vec<Ipv4Addr> = d.igmp.joined.iter().copied().filter(|g| m.group.is_unspecified() || *g == m.group).collect();
                for g in groups {
                    self.igmp_send_from(t, d, ifc, Igmp::report(g), g);
                }
            }
            return Some(Ok(()));
        }
        if ip.dst == ALL_HOSTS || d.igmp.joined.contains(&ip.dst) {
            return Some(self.local_in(t, d, ifc, ip));
        }
        Some(Err(format!("{} has not joined {}", d.name, ip.dst)))
    }
}
