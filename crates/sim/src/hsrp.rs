//! HSRP (RFC 2281): routers sharing a virtual gateway address.
//!
//! Computed from the topology like the other protocols: among the interfaces of one subnet
//! that carry the same group and are up, the highest priority (then the highest address) is
//! active, the next one standby, the rest listen. The active router answers ARP for the virtual
//! address with the group's virtual MAC and takes frames sent to it, so hosts keep one gateway
//! while routers come and go. Hellos go on the wire every 3 s (see `control`).
//!
//! Preemption (RFC 2281 §5.4): the simulator remembers which router was active in each group.
//! While that router stays in the group it keeps the role, even when a router with a higher
//! priority appears, unless that router has `standby N preempt`; then the higher priority takes
//! over. When nobody was active before (the lab just opened), the highest priority wins.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net, MacAddr};
use netlabcraft_model::{Device, DeviceKind, End, Standby, Topology};

use crate::{L3If, SVI_PORT, port_up, up_ifaces};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Standby,
    Listen,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Active => "active",
            State::Standby => "standby",
            State::Listen => "listening",
        }
    }
}

/// One router's place in one group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub iface: String,
    pub group: u8,
    pub virtual_ip: Ipv4Addr,
    pub state: State,
    pub priority: u8,
    pub preempt: bool,
    pub active: Option<Ipv4Addr>,
    pub standby: Option<Ipv4Addr>,
    /// 1, or 2 with `standby version 2`.
    pub version: u8,
    /// The configured priority, before tracked interfaces lowered it to `priority`.
    pub configured_priority: u8,
    /// Tracked interfaces that are down now.
    pub tracked_down: Vec<String>,
}

/// Where version 2 hellos go (version 1 uses 224.0.0.2).
pub const HSRP2_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 102);

impl Role {
    /// Version 1: 0000.0c07.acXX; version 2: 0000.0c9f.fXXX.
    pub fn virtual_mac(&self) -> MacAddr {
        if self.version == 2 { MacAddr([0x00, 0x00, 0x0c, 0x9f, 0xf0, self.group]) } else { netlabcraft_wire::control::hsrp_mac(self.group) }
    }
}

/// Whether an interface (by name, as typed in `standby … track`) is up.
pub(crate) fn tracked_up(t: &Topology, d: &Device, name: &str) -> bool {
    match d.port_index(name) {
        Some(i) => d.ports.get(i).is_some_and(|p| !p.shutdown && !p.err_disabled) && port_up(t, End { device: d.id, port: i }),
        // A VLAN interface or subinterface: up when it is among the device's working ones.
        None => up_ifaces(t, d).iter().any(|i| i.name.eq_ignore_ascii_case(name)),
    }
}

/// The priority after tracked interfaces that are down took their decrements, and those
/// interfaces.
pub fn effective_priority(t: &Topology, d: &Device, s: &Standby) -> (u8, Vec<String>) {
    let mut p = s.priority;
    let mut down = Vec::new();
    for (name, dec) in &s.track {
        if !tracked_up(t, d, name) {
            p = p.saturating_sub(*dec);
            down.push(name.clone());
        }
    }
    (p, down)
}

/// The groups an interface carries.
pub fn standby_of<'a>(d: &'a Device, ifc: &L3If) -> &'a [Standby] {
    if ifc.port == SVI_PORT {
        return ifc.vlan.and_then(|v| d.svis.iter().find(|s| s.vlan.get() == v)).map_or(&[], |s| s.standby.as_slice());
    }
    let Some(p) = d.ports.get(ifc.port) else { return &[] };
    match ifc.sub {
        Some(n) => p.subif(n).map_or(&[], |s| s.standby.as_slice()),
        None => p.standby.as_slice(),
    }
}

/// Who was active in each group (subnet, group) at the last election.
pub type Actives = BTreeMap<(Ipv4Net, u8), Ipv4Addr>;

/// Orders a group's members (sorted best first by priority, then address) so the one that
/// should be active comes first: the previous active router keeps the role unless a better
/// member may preempt it. `preempts(i)` says whether member `i` preempts.
pub fn keep_active<T, A: PartialEq>(members: &mut [T], prev: Option<A>, addr: impl Fn(&T) -> A, preempts: impl Fn(&T) -> bool) {
    let Some(prev) = prev else { return };
    let Some(at) = members.iter().position(|m| addr(m) == prev) else { return };
    // Someone ahead of the active router that preempts takes over; else it stays first.
    if members.iter().take(at).any(&preempts) {
        return;
    }
    if let Some(head) = members.get_mut(..=at) {
        head.rotate_right(1);
    }
}

/// Every device's roles, for a lab opened just now (nobody active before).
pub fn compute(t: &Topology) -> BTreeMap<DeviceId, Vec<Role>> {
    compute_with(t, &Actives::new()).0
}

/// Every device's roles, given who was active before; and who is active now.
pub fn compute_with(t: &Topology, prev: &Actives) -> (BTreeMap<DeviceId, Vec<Role>>, Actives) {
    // (subnet, group) → members: (device, interface, its address, settings).
    // Each member: its settings with the priority tracking left it, the configured one, the
    // tracked interfaces down, and its HSRP version.
    #[allow(clippy::type_complexity)]
    let mut groups: BTreeMap<(Ipv4Net, u8), Vec<(DeviceId, L3If, Standby, u8, Vec<String>, u8)>> = BTreeMap::new();
    for d in t.devices.iter().filter(|d| matches!(d.kind, DeviceKind::Router | DeviceKind::Switch)) {
        for ifc in up_ifaces(t, d) {
            let version = if d.mgmt.hsrp_v2.iter().any(|n| n.eq_ignore_ascii_case(&ifc.name)) { 2 } else { 1 };
            for s in standby_of(d, &ifc) {
                let (eff, down) = effective_priority(t, d, s);
                let member = Standby { priority: eff, ..s.clone() };
                groups.entry((ifc.net.network(), s.group)).or_default().push((d.id, ifc.clone(), member, s.priority, down, version));
            }
        }
    }
    let mut out: BTreeMap<DeviceId, Vec<Role>> = BTreeMap::new();
    let mut actives = Actives::new();
    for ((subnet, group), mut members) in groups {
        // Routers learn the virtual address from whoever has it configured.
        let Some(vip) = members.iter().find_map(|(_, _, s, ..)| s.ip) else { continue };
        members.sort_by_key(|(_, ifc, s, ..)| std::cmp::Reverse((s.priority, ifc.net.addr)));
        keep_active(&mut members, prev.get(&(subnet, group)).copied(), |m| m.1.net.addr, |m| m.2.preempt);
        if let Some(m) = members.first() {
            actives.insert((subnet, group), m.1.net.addr);
        }
        let active = members.first().map(|m| m.1.net.addr);
        let standby = members.get(1).map(|m| m.1.net.addr);
        for (i, (dev, ifc, s, configured, down, version)) in members.iter().enumerate() {
            let state = match i {
                0 => State::Active,
                1 => State::Standby,
                _ => State::Listen,
            };
            out.entry(*dev).or_default().push(Role {
                iface: ifc.name.clone(),
                group,
                virtual_ip: vip,
                state,
                priority: s.priority,
                preempt: s.preempt,
                active,
                standby,
                version: *version,
                configured_priority: *configured,
                tracked_down: down.clone(),
            });
        }
    }
    (out, actives)
}

/// A fingerprint of what the election depends on.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in &t.links {
        (l.a.device, l.a.port, l.b.device, l.b.port).hash(&mut h);
    }
    for d in &t.devices {
        d.id.hash(&mut h);
        for p in &d.ports {
            (p.shutdown, p.err_disabled, p.ip, &p.standby).hash(&mut h);
            for s in &p.subifs {
                (s.ip, s.vlan, s.shutdown, &s.standby).hash(&mut h);
            }
        }
        for s in &d.svis {
            (s.vlan, s.ip, s.shutdown, &s.standby).hash(&mut h);
        }
        d.mgmt.hsrp_v2.hash(&mut h);
    }
    h.finish()
}
