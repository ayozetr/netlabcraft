//! HSRP (RFC 2281): routers sharing a virtual gateway address.
//!
//! Computed from the topology like the other protocols: among the interfaces of one subnet
//! that carry the same group and are up, the highest priority (then the highest address) is
//! active, the next one standby, the rest listen. The active router answers ARP for the virtual
//! address with the group's virtual MAC and takes frames sent to it, so hosts keep one gateway
//! while routers come and go. Preemption is recorded but not timed: the election is always the
//! one the priorities give. Hellos go on the wire every 3 s (see `control`).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use packetcraft_core::{DeviceId, Ipv4Net, MacAddr};
use packetcraft_model::{Device, DeviceKind, Standby, Topology};

use crate::{L3If, SVI_PORT, up_ifaces};

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
}

impl Role {
    pub fn virtual_mac(&self) -> MacAddr {
        packetcraft_wire::control::hsrp_mac(self.group)
    }
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

/// Every device's roles.
pub fn compute(t: &Topology) -> BTreeMap<DeviceId, Vec<Role>> {
    // (subnet, group) → members: (device, interface, its address, settings).
    let mut groups: BTreeMap<(Ipv4Net, u8), Vec<(DeviceId, L3If, Standby)>> = BTreeMap::new();
    for d in t.devices.iter().filter(|d| matches!(d.kind, DeviceKind::Router | DeviceKind::Switch)) {
        for ifc in up_ifaces(t, d) {
            for s in standby_of(d, &ifc) {
                groups.entry((ifc.net.network(), s.group)).or_default().push((d.id, ifc.clone(), *s));
            }
        }
    }
    let mut out: BTreeMap<DeviceId, Vec<Role>> = BTreeMap::new();
    for ((_, group), mut members) in groups {
        // Routers learn the virtual address from whoever has it configured.
        let Some(vip) = members.iter().find_map(|(_, _, s)| s.ip) else { continue };
        members.sort_by_key(|(_, ifc, s)| std::cmp::Reverse((s.priority, ifc.net.addr)));
        let active = members.first().map(|m| m.1.net.addr);
        let standby = members.get(1).map(|m| m.1.net.addr);
        for (i, (dev, ifc, s)) in members.iter().enumerate() {
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
            });
        }
    }
    out
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
    }
    h.finish()
}
